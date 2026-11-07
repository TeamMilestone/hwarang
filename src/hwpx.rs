use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use rayon::prelude::*;

use crate::error::{HwpError, Result};

/// HWPX (ZIP-based OWPML) 파일에서 텍스트를 추출한다.
///
/// 섹션별 병렬 처리: ZIP 엔트리 I/O 후 XML 파싱을 rayon으로 병렬 수행한다.
pub fn extract_text_from_hwpx(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| HwpError::Hwpx(format!("ZIP open: {}", e)))?;
    extract_text_from_archive(&mut archive)
}

/// 이미 열린 ZIP 아카이브에서 HWPX 텍스트를 추출한다.
pub(crate) fn extract_text_from_archive<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<String> {
    // section*.xml 파일들 찾기 (정렬)
    let mut section_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| HwpError::Hwpx(format!("ZIP entry: {}", e)))?;
        let name = entry.name().to_string();
        if name.starts_with("Contents/section") && name.ends_with(".xml") {
            section_names.push(name);
        }
    }
    section_names.sort();

    // Phase 1: 모든 섹션 XML을 순차 읽기 (ZIP I/O)
    let mut section_xmls: Vec<(usize, String)> = Vec::new();
    for (idx, section_name) in section_names.iter().enumerate() {
        let mut entry = archive
            .by_name(section_name)
            .map_err(|e| HwpError::Hwpx(format!("ZIP entry '{}': {}", section_name, e)))?;

        let mut xml_data = String::new();
        entry
            .read_to_string(&mut xml_data)
            .map_err(|e| HwpError::Hwpx(format!("read section XML: {}", e)))?;

        section_xmls.push((idx, xml_data));
    }

    // Phase 2: 섹션별 XML 파싱을 병렬 수행
    let mut section_texts: Vec<(usize, String)> = section_xmls
        .into_par_iter()
        .map(|(idx, xml_data)| {
            let mut text = String::new();
            extract_section_xml(&xml_data, &mut text)?;
            Ok((idx, text))
        })
        .collect::<Result<Vec<_>>>()?;

    // Phase 3: 섹션 순서대로 병합
    section_texts.sort_unstable_by_key(|(i, _)| *i);
    let text = section_texts
        .into_iter()
        .map(|(_, t)| t)
        .collect::<String>();

    Ok(text)
}

/// 섹션 XML에서 텍스트를 추출한다.
/// <hp:p> → 줄바꿈, <hp:t> → 텍스트 수집, <hp:tbl> → 표 (표 안의 표도 바깥 셀에 넣는다)
fn extract_section_xml(xml: &str, text: &mut String) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut in_t_tag = false;
    let mut out = TextBuilder::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"t" => in_t_tag = true,
                b"tbl" => out.table_start(),
                b"tr" => out.row_start(),
                b"tc" => out.cell_start(),
                b"p" => out.para_start(),
                _ => {}
            },
            // 빈 요소는 열고 닫은 것으로 본다. 빈 <hp:t/> 는 글이 없다
            Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"tc" {
                    out.cell_start();
                    out.cell_end();
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"t" => in_t_tag = false,
                b"p" => out.para_end(),
                b"tc" => out.cell_end(),
                b"tr" => out.row_end(),
                b"tbl" => out.table_end(),
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_t_tag {
                    let t = e
                        .unescape()
                        .map_err(|err| HwpError::Hwpx(format!("XML unescape: {}", err)))?;
                    out.push_text(&t);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::Hwpx(format!(
                    "XML parse error at {}: {}",
                    reader.error_position(),
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    text.push_str(&out.finish());
    Ok(())
}

/// 표 안의 표까지 다루는 본문 모음.
///
/// 열린 표를 바깥에서 안쪽 순서로 쌓고, 표가 끝나면 그 표를 감싼 셀(없으면 본문)에 쓴다.
/// 0.4.0 까지는 표 상태를 하나만 들고 있어, 안쪽 표가 시작될 때 바깥 셀에 모은 글을 지웠다
/// (문서 전체를 1×1 표로 감싼 공고문은 본문이 통째로 빠졌다).
#[derive(Default)]
struct TextBuilder {
    text: String,
    para_has_text: bool,
    tables: Vec<OpenTable>,
}

#[derive(Default)]
struct OpenTable {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    /// 열린 셀의 글
    cell: Option<String>,
    cell_para_has_text: bool,
}

impl TextBuilder {
    fn table_start(&mut self) {
        self.tables.push(OpenTable::default());
    }

    fn row_start(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            t.row.clear();
        }
    }

    fn cell_start(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            t.cell = Some(String::new());
            t.cell_para_has_text = false;
        }
    }

    fn para_start(&mut self) {
        match self.tables.last_mut() {
            Some(t) => t.cell_para_has_text = false,
            None => self.para_has_text = false,
        }
    }

    fn para_end(&mut self) {
        match self.tables.last_mut() {
            Some(t) => {
                if let Some(cell) = t.cell.as_mut() {
                    if t.cell_para_has_text {
                        cell.push('\n');
                    }
                }
            }
            None => {
                if self.para_has_text {
                    self.text.push('\n');
                } else {
                    self.text.push_str("\n\n");
                }
            }
        }
    }

    fn push_text(&mut self, s: &str) {
        match self.tables.last_mut() {
            Some(t) => {
                if let Some(cell) = t.cell.as_mut() {
                    if !s.is_empty() {
                        t.cell_para_has_text = true;
                    }
                    cell.push_str(s);
                }
            }
            None => {
                if !s.is_empty() {
                    self.para_has_text = true;
                }
                self.text.push_str(s);
            }
        }
    }

    fn cell_end(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            if let Some(cell) = t.cell.take() {
                t.row.push(cell.trim_end_matches('\n').to_string());
            }
        }
    }

    fn row_end(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            if !t.row.is_empty() {
                let row = std::mem::take(&mut t.row);
                t.rows.push(row);
            }
        }
    }

    fn table_end(&mut self) {
        let Some(table) = self.tables.pop() else {
            return;
        };
        let mut rendered = String::new();
        emit_hwpx_markdown_table(&table.rows, &mut rendered);
        self.write_block(&rendered);
    }

    /// 끝난 표를 감싼 셀에 쓴다. 감싼 셀이 없으면 본문에 쓴다.
    fn write_block(&mut self, block: &str) {
        match self.tables.iter_mut().rev().find_map(|t| t.cell.as_mut()) {
            Some(cell) => cell.push_str(block),
            None => self.text.push_str(block),
        }
    }

    fn finish(mut self) -> String {
        // 닫히지 않은 표(손상된 XML)도 모은 글은 버리지 않는다
        while !self.tables.is_empty() {
            self.cell_end();
            self.row_end();
            self.table_end();
        }
        self.text
    }
}

/// HWPX 표 데이터를 마크다운 테이블로 출력
fn emit_hwpx_markdown_table(rows: &[Vec<String>], text: &mut String) {
    if rows.is_empty() {
        return;
    }
    // 1×1 표는 테두리 상자다 — 줄을 살려 본문처럼 쓴다 (0.4.1)
    if let [row] = rows {
        if let [cell] = row.as_slice() {
            push_box(cell, text);
            return;
        }
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return;
    }

    for (i, row) in rows.iter().enumerate() {
        text.push('|');
        for j in 0..col_count {
            let cell = row.get(j).map(|s| s.as_str()).unwrap_or("");
            let escaped = escape_markdown_cell(cell);
            text.push(' ');
            text.push_str(&escaped);
            text.push_str(" |");
        }
        text.push('\n');

        // 첫 행 뒤에 구분선
        if i == 0 {
            text.push('|');
            for _ in 0..col_count {
                text.push_str(" --- |");
            }
            text.push('\n');
        }
    }
}

/// HWPML (순수 XML, ZIP 없음) 파일에서 텍스트를 추출한다.
/// 구조: `HWPML → BODY → SECTION → P → TEXT → CHAR`
pub fn extract_text_from_hwpml(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut xml_data = String::new();
    file.read_to_string(&mut xml_data)
        .map_err(|e| HwpError::Hwpx(format!("read HWPML: {}", e)))?;

    // quick-xml은 DTD 엔티티를 지원하지 않으므로 &nbsp; → &#160; 치환
    let xml_data = xml_data.replace("&nbsp;", "&#160;");

    let mut text = String::new();
    extract_hwpml_xml(&xml_data, &mut text)?;
    Ok(text)
}

/// HWPML XML에서 텍스트를 추출한다.
/// <P> → 줄바꿈, <CHAR> → 텍스트 수집, <TABLE> → 표 (표 안의 표도 바깥 셀에 넣는다)
fn extract_hwpml_xml(xml: &str, text: &mut String) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut in_char_tag = false;
    let mut out = TextBuilder::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"CHAR" => in_char_tag = true,
                b"TABLE" => out.table_start(),
                b"ROW" => out.row_start(),
                b"CELL" => out.cell_start(),
                b"P" => out.para_start(),
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"CHAR" => in_char_tag = false,
                b"P" => out.para_end(),
                b"CELL" => out.cell_end(),
                b"ROW" => out.row_end(),
                b"TABLE" => out.table_end(),
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_char_tag {
                    let t = e
                        .unescape()
                        .map_err(|err| HwpError::Hwpx(format!("HWPML unescape: {}", err)))?;
                    out.push_text(&t);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::Hwpx(format!(
                    "HWPML parse error at {}: {}",
                    reader.error_position(),
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    text.push_str(&out.finish());
    Ok(())
}

/// 1×1 표(테두리 상자)의 글을 본문처럼 쓴다. 빈 상자는 쓰지 않는다
pub(crate) fn push_box(cell: &str, text: &mut String) {
    let body = cell.trim_matches('\n');
    if body.trim().is_empty() {
        return;
    }
    text.push_str(body);
    text.push('\n');
}

/// 마크다운 셀 텍스트 이스케이프: 줄바꿈 → 공백, | → \|
fn escape_markdown_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section_xml_simple() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p>
    <hp:run>
      <hp:t>안녕하세요</hp:t>
    </hp:run>
  </hp:p>
  <hp:p>
    <hp:run>
      <hp:t>테스트</hp:t>
    </hp:run>
  </hp:p>
</hp:sec>"#;

        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(text.contains("안녕하세요"));
        assert!(text.contains("테스트"));
    }

    #[test]
    fn test_extract_hwpml_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE HWPML [<!ENTITY nbsp "&#160;">]>
<HWPML Version="2.1">
<HEAD SecCnt="1"><DOCSUMMARY><TITLE>테스트</TITLE></DOCSUMMARY></HEAD>
<BODY>
<SECTION>
<P ParaShape="0"><TEXT CharShape="0"><CHAR>안녕하세요</CHAR></TEXT></P>
<P ParaShape="0"><TEXT CharShape="0"><CHAR>HWPML 테스트</CHAR></TEXT></P>
</SECTION>
</BODY>
</HWPML>"#;

        let mut text = String::new();
        extract_hwpml_xml(xml, &mut text).unwrap();
        assert!(text.contains("안녕하세요"), "got: {:?}", text);
        assert!(text.contains("HWPML 테스트"), "got: {:?}", text);
    }

    #[test]
    fn test_extract_section_xml_multi_run() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p>
    <hp:run>
      <hp:t>Hello </hp:t>
    </hp:run>
    <hp:run>
      <hp:t>World</hp:t>
    </hp:run>
  </hp:p>
</hp:sec>"#;

        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn test_extract_section_xml_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
</hp:sec>"#;
        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(text.trim().is_empty());
    }

    #[test]
    fn test_extract_section_xml_invalid_xml() {
        let xml = "this is not valid xml <<<<";
        let mut text = String::new();
        let result = extract_section_xml(xml, &mut text);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_text_from_hwpx_nonexistent_file() {
        let path = std::path::Path::new("/tmp/nonexistent_file_12345.hwpx");
        let result = extract_text_from_hwpx(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_text_from_hwpx_not_a_zip() {
        // 임시 파일에 ZIP이 아닌 데이터 기록
        let dir = std::env::temp_dir();
        let path = dir.join("test_not_a_zip.hwpx");
        std::fs::write(&path, b"this is not a zip file").unwrap();
        let result = extract_text_from_hwpx(&path);
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_escape_markdown_cell_hwpx() {
        assert_eq!(escape_markdown_cell(""), "");
        assert_eq!(escape_markdown_cell("hello"), "hello");
        assert_eq!(escape_markdown_cell("a|b"), "a\\|b");
        assert_eq!(escape_markdown_cell("x\ny"), "x y");
    }

    #[test]
    fn test_emit_hwpx_markdown_table_empty() {
        let rows: Vec<Vec<String>> = vec![];
        let mut text = String::new();
        emit_hwpx_markdown_table(&rows, &mut text);
        assert!(text.is_empty());
    }

    #[test]
    fn test_emit_hwpx_markdown_table_basic() {
        let rows = vec![
            vec!["A".to_string(), "B".to_string()],
            vec!["C".to_string(), "D".to_string()],
        ];
        let mut text = String::new();
        emit_hwpx_markdown_table(&rows, &mut text);
        assert!(text.contains("| A | B |"));
        assert!(text.contains("| --- | --- |"));
        assert!(text.contains("| C | D |"));
    }

    #[test]
    fn test_nested_table_keeps_outer_cell_text() {
        // 공고문 전체를 1×1 표로 감싸고 그 안에 2열 표가 있는 문서 (나라장터 R26BK01769227)
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:tbl><hp:tr><hp:tc><hp:subList>
<hp:p><hp:run><hp:t>한국농수산대학교 제2026-81호</hp:t></hp:run></hp:p>
<hp:p><hp:run><hp:t>1. 입찰개요</hp:t></hp:run></hp:p>
<hp:p><hp:run><hp:tbl><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:t>계약보증금</hp:t></hp:run></hp:p></hp:subList></hp:tc><hp:tc><hp:subList><hp:p><hp:run><hp:t>10%</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>
<hp:p><hp:run><hp:t>2. 입찰참가자격</hp:t></hp:run></hp:p>
</hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p></hp:sec>"#;
        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(
            text.starts_with("한국농수산대학교 제2026-81호\n1. 입찰개요\n| 계약보증금 | 10% |\n| --- | --- |\n2. 입찰참가자격\n"),
            "got: {text:?}"
        );
    }

    #[test]
    fn test_nested_table_inside_grid_cell_is_inlined() {
        // 여러 칸 표 안의 표는 바깥 칸 한 줄에 들어간다 (HWP 바이너리와 같다)
        let xml = r#"<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:tbl>
<hp:tr><hp:tc><hp:p><hp:run><hp:t>구분</hp:t></hp:run></hp:p></hp:tc><hp:tc><hp:p><hp:run><hp:t>내용</hp:t></hp:run></hp:p></hp:tc></hp:tr>
<hp:tr><hp:tc><hp:p><hp:run><hp:t>보증</hp:t></hp:run></hp:p></hp:tc><hp:tc><hp:p><hp:run><hp:t>앞</hp:t></hp:run></hp:p><hp:p><hp:run><hp:tbl><hp:tr><hp:tc><hp:p><hp:run><hp:t>A</hp:t></hp:run></hp:p></hp:tc><hp:tc><hp:p><hp:run><hp:t>B</hp:t></hp:run></hp:p></hp:tc></hp:tr></hp:tbl></hp:run></hp:p><hp:p><hp:run><hp:t>뒤</hp:t></hp:run></hp:p></hp:tc></hp:tr>
</hp:tbl></hp:run></hp:p></hp:sec>"#;
        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(
            text.contains("| 구분 | 내용 |\n| --- | --- |\n"),
            "got: {text:?}"
        );
        assert!(
            text.contains(r"| 보증 | 앞 \| A \| B \| \| --- \| --- \| 뒤 |"),
            "got: {text:?}"
        );
    }

    #[test]
    fn test_single_cell_table_is_a_box() {
        let rows = vec![vec![
            "※ 공동수급은 허용하지 않습니다.\n면세사업자는 부가세 포함".to_string(),
        ]];
        let mut text = String::new();
        emit_hwpx_markdown_table(&rows, &mut text);
        assert_eq!(
            text,
            "※ 공동수급은 허용하지 않습니다.\n면세사업자는 부가세 포함\n"
        );
        let mut empty = String::new();
        emit_hwpx_markdown_table(&[vec!["  ".to_string()]], &mut empty);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_hwpml_nested_table_keeps_outer_cell_text() {
        let xml = r#"<HWPML><BODY><SECTION><P><TEXT><TABLE><ROW><CELL><PARALIST>
<P><TEXT><CHAR>바깥 앞</CHAR></TEXT></P>
<P><TEXT><TABLE><ROW><CELL><PARALIST><P><TEXT><CHAR>가</CHAR></TEXT></P></PARALIST></CELL><CELL><PARALIST><P><TEXT><CHAR>나</CHAR></TEXT></P></PARALIST></CELL></ROW></TABLE></TEXT></P>
<P><TEXT><CHAR>바깥 뒤</CHAR></TEXT></P>
</PARALIST></CELL></ROW></TABLE></TEXT></P></SECTION></BODY></HWPML>"#;
        let mut text = String::new();
        extract_hwpml_xml(xml, &mut text).unwrap();
        assert!(
            text.starts_with("바깥 앞\n| 가 | 나 |\n| --- | --- |\n바깥 뒤\n"),
            "got: {text:?}"
        );
    }

    #[test]
    fn test_extract_section_xml_table() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p>
    <hp:run>
      <hp:tbl>
        <hp:tr>
          <hp:tc><hp:p><hp:run><hp:t>셀1</hp:t></hp:run></hp:p></hp:tc>
          <hp:tc><hp:p><hp:run><hp:t>셀2</hp:t></hp:run></hp:p></hp:tc>
        </hp:tr>
      </hp:tbl>
    </hp:run>
  </hp:p>
</hp:sec>"#;
        let mut text = String::new();
        extract_section_xml(xml, &mut text).unwrap();
        assert!(text.contains("셀1"), "got: {text:?}");
        assert!(text.contains("셀2"), "got: {text:?}");
    }
}
