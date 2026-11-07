use std::collections::HashMap;
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

    // 투명 표를 가리려고 문서 머리(header.xml)의 테두리 정의를 읽는다 (0.5.0). 없거나 못 읽으면 모두 "모름"
    let borders = read_header_borders(archive);

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
            extract_section_xml_with(&xml_data, &borders, &mut text)?;
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

/// 칸의 네 변에 선이 보이는가
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// 테두리/배경 id → 변마다 선이 보이는가
pub(crate) type BorderMap = HashMap<String, Sides>;

fn read_header_borders<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> BorderMap {
    let Ok(mut entry) = archive.by_name("Contents/header.xml") else {
        return BorderMap::new();
    };
    let mut xml = String::new();
    if entry.read_to_string(&mut xml).is_err() {
        return BorderMap::new();
    }
    parse_border_fills(&xml)
}

/// `<hh:borderFill id="3"><hh:leftBorder type="NONE"/>…<hh:bottomBorder type="NONE"/></hh:borderFill>` (HWPX)
/// 또는 `<BORDERFILL Id="3"><LEFTBORDER Type="None"/>…</BORDERFILL>` (HWPML). 네 변이 모두 있고 모두 없음이면 투명
fn parse_border_fills(xml: &str) -> BorderMap {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut map = BorderMap::new();
    let mut current: Option<(String, u8, Sides)> = None; // (id, 읽은 변 수, 보이는 변)
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.local_name();
                let name = name.as_ref();
                if name.eq_ignore_ascii_case(b"borderFill") {
                    current = attr(e, &[b"id", b"Id"]).map(|id| (id, 0, Sides::default()));
                } else if let Some((_, count, sides)) = current.as_mut() {
                    let slot = if name.eq_ignore_ascii_case(b"leftBorder") {
                        Some(&mut sides.left)
                    } else if name.eq_ignore_ascii_case(b"rightBorder") {
                        Some(&mut sides.right)
                    } else if name.eq_ignore_ascii_case(b"topBorder") {
                        Some(&mut sides.top)
                    } else if name.eq_ignore_ascii_case(b"bottomBorder") {
                        Some(&mut sides.bottom)
                    } else {
                        None
                    };
                    if let Some(slot) = slot {
                        *count += 1;
                        let kind = attr(e, &[b"type", b"Type"]).unwrap_or_default();
                        *slot = !kind.eq_ignore_ascii_case("none");
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"borderFill") {
                    if let Some((id, count, sides)) = current.take() {
                        if count == 4 {
                            map.insert(id, sides);
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

fn attr(e: &quick_xml::events::BytesStart, names: &[&[u8]]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        names
            .contains(&a.key.local_name().as_ref())
            .then(|| String::from_utf8_lossy(&a.value).into_owned())
    })
}

/// 칸의 변마다 선이 보이는가. 테두리 정의를 모르면 None
fn cell_visibility(
    e: &quick_xml::events::BytesStart,
    names: &[&[u8]],
    borders: &BorderMap,
) -> Option<Sides> {
    attr(e, names).and_then(|id| borders.get(&id).copied())
}

/// 섹션 XML에서 텍스트를 추출한다.
/// <hp:p> → 줄바꿈, <hp:t> → 텍스트 수집, <hp:tbl> → 표 (표 안의 표도 바깥 셀에 넣는다)
#[cfg(test)]
fn extract_section_xml(xml: &str, text: &mut String) -> Result<()> {
    extract_section_xml_with(xml, &BorderMap::new(), text)
}

fn extract_section_xml_with(xml: &str, borders: &BorderMap, text: &mut String) -> Result<()> {
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
                b"tc" => out.cell_start(cell_visibility(e, &[b"borderFillIDRef"], borders)),
                b"p" => out.para_start(),
                _ => {}
            },
            // 빈 요소는 열고 닫은 것으로 본다. 빈 <hp:t/> 는 글이 없다
            Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"tc" {
                    out.cell_start(cell_visibility(e, &[b"borderFillIDRef"], borders));
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
    /// 칸 테두리가 보이는가 (rows·row 와 같은 모양, 모르면 None)
    rows_vis: Vec<Vec<Option<Sides>>>,
    row_vis: Vec<Option<Sides>>,
    cell_vis: Option<Sides>,
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
            t.row_vis.clear();
        }
    }

    fn cell_start(&mut self, visible: Option<Sides>) {
        if let Some(t) = self.tables.last_mut() {
            t.cell = Some(String::new());
            t.cell_para_has_text = false;
            t.cell_vis = visible;
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
                t.row_vis.push(t.cell_vis.take());
            }
        }
    }

    fn row_end(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            if !t.row.is_empty() {
                let row = std::mem::take(&mut t.row);
                t.rows.push(row);
                let vis = std::mem::take(&mut t.row_vis);
                t.rows_vis.push(vis);
            }
        }
    }

    fn table_end(&mut self) {
        let Some(table) = self.tables.pop() else {
            return;
        };
        let mut rendered = String::new();
        let layout: Vec<bool> = table.rows_vis.iter().map(|v| is_layout_row(v)).collect();
        emit_table(&table.rows, &layout, &mut rendered);
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
    // 테두리 정의(HEAD 의 BORDERFILL)는 본문보다 앞에 있다 — 같은 글에서 먼저 읽어 둔다
    let borders = parse_border_fills(xml);
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
                b"CELL" => out.cell_start(cell_visibility(e, &[b"BorderFill"], &borders)),
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

/// 본문 줄로 쓸 표 줄: 모든 칸의 테두리를 알고, 이웃한 칸 사이 세로선이 하나도 보이지 않는다.
/// 목록 배치용 투명 표와 바깥 테두리만 있는 상자가 여기 든다. 칸 사이 세로선이 있으면 실제 표다
pub(crate) fn is_layout_row(cells: &[Option<Sides>]) -> bool {
    !cells.is_empty()
        && cells.iter().all(Option::is_some)
        && cells
            .windows(2)
            .all(|w| matches!((w[0], w[1]), (Some(a), Some(b)) if !a.right && !b.left))
}

/// 표를 쓴다. 작성자가 목록 배치를 맞추려고 쓴 줄(`is_layout_row`)은 본문 줄로 쓰고,
/// 칸 사이 세로선이 보이는 줄이 이어진 부분만 마크다운 표로 쓴다 (0.5.0). 그런 줄이 없으면 전과 같다
pub(crate) fn emit_table(rows: &[Vec<String>], layout: &[bool], text: &mut String) {
    let layout: Vec<bool> = (0..rows.len())
        .map(|i| layout.get(i).copied().unwrap_or(false))
        .collect();
    if !layout.contains(&true) {
        emit_hwpx_markdown_table(rows, text);
        return;
    }
    let mut i = 0;
    while i < rows.len() {
        if layout[i] {
            emit_layout_row(&rows[i], text);
            i += 1;
            continue;
        }
        let end = (i..rows.len()).find(|&k| layout[k]).unwrap_or(rows.len());
        emit_hwpx_markdown_table(&rows[i..end], text);
        i = end;
    }
}

/// 투명한 줄 하나: 채운 칸을 차례로 잇는다. "1." "○" 같은 짧은 기호 칸 뒤는 띄어 쓰고,
/// 여러 줄인 칸 사이는 줄을 바꾼다
fn emit_layout_row(row: &[String], text: &mut String) {
    let mut line = String::new();
    for cell in row
        .iter()
        .map(|c| c.trim_matches(|ch: char| ch == '\n' || ch == ' '))
        .filter(|c| !c.is_empty())
    {
        if !line.is_empty() {
            let marker = !line.contains('\n') && line.chars().count() <= 6;
            let multi = line.contains('\n') || cell.contains('\n');
            line.push(if multi && !marker { '\n' } else { ' ' });
        }
        line.push_str(cell);
    }
    if !line.is_empty() {
        text.push_str(&line);
        text.push('\n');
    }
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
    fn test_invisible_rows_become_lines_visible_rows_stay_a_table() {
        let header = r#"<hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"><hh:borderFills>
<hh:borderFill id="1"><hh:leftBorder type="NONE"/><hh:rightBorder type="NONE"/><hh:topBorder type="NONE"/><hh:bottomBorder type="NONE"/></hh:borderFill>
<hh:borderFill id="2"><hh:leftBorder type="SOLID"/><hh:rightBorder type="SOLID"/><hh:topBorder type="SOLID"/><hh:bottomBorder type="SOLID"/></hh:borderFill>
</hh:borderFills></hh:head>"#;
        let borders = parse_border_fills(header);
        assert_eq!(borders.get("1"), Some(&Sides::default()));
        assert!(borders
            .get("2")
            .is_some_and(|s| s.left && s.right && s.top && s.bottom));
        let tc = |bf: u8, t: &str| {
            format!(
                r#"<hp:tc borderFillIDRef="{bf}"><hp:subList><hp:p><hp:run><hp:t>{t}</hp:t></hp:run></hp:p></hp:subList></hp:tc>"#
            )
        };
        let xml = format!(
            r#"<hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:tbl>
<hp:tr>{}{}{}</hp:tr>
<hp:tr>{}{}{}</hp:tr>
<hp:tr>{}{}{}</hp:tr>
<hp:tr>{}{}{}</hp:tr>
</hp:tbl></hp:run></hp:p></hp:sec>"#,
            tc(1, "○"),
            tc(1, "공고명:"),
            tc(1, "현장 체험활동 위탁 용역"),
            tc(1, "1."),
            tc(1, "금품을 요구하지 않겠습니다."),
            tc(1, ""),
            tc(2, "구분"),
            tc(2, "금액"),
            tc(2, "비고"),
            tc(2, "계약보증금"),
            tc(2, "10%"),
            tc(2, "-"),
        );
        let mut text = String::new();
        extract_section_xml_with(&xml, &borders, &mut text).unwrap();
        assert!(
            text.starts_with("○ 공고명: 현장 체험활동 위탁 용역\n1. 금품을 요구하지 않겠습니다.\n| 구분 | 금액 | 비고 |\n| --- | --- | --- |\n| 계약보증금 | 10% | - |\n"),
            "got: {text:?}"
        );
        // 테두리 정의를 모르면 전과 같이 표로
        let mut plain = String::new();
        extract_section_xml(&xml, &mut plain).unwrap();
        assert!(plain.starts_with("| ○ | 공고명: |"), "got: {plain:?}");
    }

    #[test]
    fn test_framed_box_without_inner_lines_is_text() {
        // 바깥 테두리만 있는 상자(왼쪽 칸은 왼쪽 선, 오른쪽 칸은 오른쪽 선만)는 본문, 칸 사이 선이 있으면 표
        let left = Sides {
            left: true,
            ..Sides::default()
        };
        let right = Sides {
            right: true,
            ..Sides::default()
        };
        let grid = Sides {
            left: true,
            right: true,
            top: true,
            bottom: true,
        };
        assert!(is_layout_row(&[Some(left), Some(right)]));
        assert!(!is_layout_row(&[Some(grid), Some(grid)]));
        assert!(!is_layout_row(&[Some(left), None]));
        assert!(is_layout_row(&[Some(grid)]));
    }

    #[test]
    fn test_hwpml_invisible_rows() {
        let xml = r#"<HWPML><HEAD><BORDERFILLLIST><BORDERFILL Id="1"><LEFTBORDER Type="None"/><RIGHTBORDER Type="None"/><TOPBORDER Type="None"/><BOTTOMBORDER Type="None"/></BORDERFILL></BORDERFILLLIST></HEAD><BODY><SECTION><P><TEXT><TABLE>
<ROW><CELL BorderFill="1"><PARALIST><P><TEXT><CHAR>가.</CHAR></TEXT></P></PARALIST></CELL><CELL BorderFill="1"><PARALIST><P><TEXT><CHAR>용역명: 시설관리</CHAR></TEXT></P></PARALIST></CELL></ROW>
<ROW><CELL BorderFill="1"><PARALIST><P><TEXT><CHAR>나.</CHAR></TEXT></P></PARALIST></CELL><CELL BorderFill="1"><PARALIST><P><TEXT><CHAR>기간: 1년</CHAR></TEXT></P></PARALIST></CELL></ROW>
</TABLE></TEXT></P></SECTION></BODY></HWPML>"#;
        let mut text = String::new();
        extract_hwpml_xml(xml, &mut text).unwrap();
        assert!(
            text.starts_with("가. 용역명: 시설관리\n나. 기간: 1년\n"),
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
