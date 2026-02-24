use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::{HwpError, Result};

/// HWPX (ZIP-based OWPML) 파일에서 텍스트를 추출한다.
pub fn extract_text_from_hwpx(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| HwpError::Hwpx(format!("ZIP open: {}", e)))?;

    // section*.xml 파일들 찾기 (정렬)
    let mut section_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| HwpError::Hwpx(format!("ZIP entry: {}", e)))?;
        let name = entry.name().to_string();
        // Contents/section0.xml, Contents/section1.xml, ...
        if name.starts_with("Contents/section") && name.ends_with(".xml") {
            section_names.push(name);
        }
    }
    section_names.sort();

    let mut text = String::new();
    for section_name in &section_names {
        let mut entry = archive
            .by_name(section_name)
            .map_err(|e| HwpError::Hwpx(format!("ZIP entry '{}': {}", section_name, e)))?;

        let mut xml_data = String::new();
        entry
            .read_to_string(&mut xml_data)
            .map_err(|e| HwpError::Hwpx(format!("read section XML: {}", e)))?;

        extract_section_xml(&xml_data, &mut text)?;
    }

    Ok(text)
}

/// 섹션 XML에서 텍스트를 추출한다.
/// <hp:p> → 줄바꿈, <hp:t> → 텍스트 수집
fn extract_section_xml(xml: &str, text: &mut String) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut in_t_tag = false;
    let mut para_has_text = false;
    let mut buf = Vec::new();

    // 표 추적
    let mut in_table = false;
    let mut in_tc = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell_text = String::new();
    let mut tc_para_has_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if name == b"t" {
                    in_t_tag = true;
                } else if name == b"tbl" {
                    in_table = true;
                    table_rows.clear();
                } else if name == b"tc" {
                    in_tc = true;
                    current_cell_text.clear();
                    tc_para_has_text = false;
                } else if name == b"tr" && in_table {
                    current_row.clear();
                } else if name == b"p" {
                    if in_tc {
                        tc_para_has_text = false;
                    } else {
                        para_has_text = false;
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if name == b"t" {
                    in_t_tag = false;
                } else if name == b"p" {
                    if in_tc {
                        if tc_para_has_text {
                            current_cell_text.push('\n');
                        }
                    } else if !in_table {
                        if para_has_text {
                            text.push('\n');
                        } else {
                            text.push_str("\n\n");
                        }
                    }
                } else if name == b"tc" {
                    // 셀 텍스트 끝의 줄바꿈 제거
                    let trimmed = current_cell_text.trim_end_matches('\n').to_string();
                    current_row.push(trimmed);
                    in_tc = false;
                } else if name == b"tr" && in_table {
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                } else if name == b"tbl" {
                    emit_hwpx_markdown_table(&table_rows, text);
                    table_rows.clear();
                    in_table = false;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_t_tag {
                    let t = e
                        .unescape()
                        .map_err(|err| HwpError::Hwpx(format!("XML unescape: {}", err)))?;
                    if in_tc {
                        if !t.is_empty() {
                            tc_para_has_text = true;
                        }
                        current_cell_text.push_str(&t);
                    } else if !in_table {
                        if !t.is_empty() {
                            para_has_text = true;
                        }
                        text.push_str(&t);
                    }
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

    Ok(())
}

/// HWPX 표 데이터를 마크다운 테이블로 출력
fn emit_hwpx_markdown_table(rows: &[Vec<String>], text: &mut String) {
    if rows.is_empty() {
        return;
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
/// <P> → 줄바꿈, <CHAR> → 텍스트 수집
fn extract_hwpml_xml(xml: &str, text: &mut String) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut in_char_tag = false;
    let mut para_has_text = false;
    let mut buf = Vec::new();

    // 표 추적
    let mut in_table = false;
    let mut in_cell = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell_text = String::new();
    let mut cell_para_has_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if name == b"CHAR" {
                    in_char_tag = true;
                } else if name == b"TABLE" {
                    in_table = true;
                    table_rows.clear();
                } else if name == b"CELL" {
                    in_cell = true;
                    current_cell_text.clear();
                    cell_para_has_text = false;
                } else if name == b"ROW" && in_table {
                    current_row.clear();
                } else if name == b"P" {
                    if in_cell {
                        cell_para_has_text = false;
                    } else {
                        para_has_text = false;
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if name == b"CHAR" {
                    in_char_tag = false;
                } else if name == b"P" {
                    if in_cell {
                        if cell_para_has_text {
                            current_cell_text.push('\n');
                        }
                    } else if !in_table {
                        if para_has_text {
                            text.push('\n');
                        } else {
                            text.push_str("\n\n");
                        }
                    }
                } else if name == b"CELL" {
                    let trimmed = current_cell_text.trim_end_matches('\n').to_string();
                    current_row.push(trimmed);
                    in_cell = false;
                } else if name == b"ROW" && in_table {
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                } else if name == b"TABLE" {
                    emit_hwpx_markdown_table(&table_rows, text);
                    table_rows.clear();
                    in_table = false;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_char_tag {
                    let t = e
                        .unescape()
                        .map_err(|err| HwpError::Hwpx(format!("HWPML unescape: {}", err)))?;
                    if in_cell {
                        if !t.is_empty() {
                            cell_para_has_text = true;
                        }
                        current_cell_text.push_str(&t);
                    } else if !in_table {
                        if !t.is_empty() {
                            para_has_text = true;
                        }
                        text.push_str(&t);
                    }
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

    Ok(())
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
