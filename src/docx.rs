use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use crate::error::{HwpError, Result};

/// DOCX (OOXML WordprocessingML) 파일에서 텍스트를 추출한다.
///
/// `word/document.xml` 본문을 추출하고, 각주(`word/footnotes.xml`)와
/// 미주(`word/endnotes.xml`)에 실제 텍스트가 있으면 뒤에 덧붙인다.
pub fn extract_text_from_docx(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| HwpError::Docx(format!("ZIP open: {}", e)))?;
    extract_text_from_archive(&mut archive)
}

/// 이미 열린 ZIP 아카이브에서 DOCX 텍스트를 추출한다.
pub(crate) fn extract_text_from_archive<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<String> {
    let document_xml = read_zip_entry(archive, "word/document.xml")?
        .ok_or_else(|| HwpError::Docx("word/document.xml not found".into()))?;

    let mut text = String::new();
    extract_document_xml(&document_xml, &mut text)?;

    // 각주/미주: 실제 텍스트가 있을 때만 덧붙인다
    for part in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(xml) = read_zip_entry(archive, part)? {
            let mut note_text = String::new();
            extract_document_xml(&xml, &mut note_text)?;
            if !note_text.trim().is_empty() {
                text.push_str(note_text.trim_end_matches('\n'));
                text.push('\n');
            }
        }
    }

    Ok(text)
}

/// ZIP 엔트리를 문자열로 읽는다. 엔트리가 없으면 `None`.
fn read_zip_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    let mut entry = match archive.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(HwpError::Docx(format!("ZIP entry '{}': {}", name, e))),
    };
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|e| HwpError::Docx(format!("read '{}': {}", name, e)))?;
    Ok(Some(xml))
}

/// 요소의 `xml:space` 속성을 읽는다. 없으면 `None`.
fn read_xml_space(e: &BytesStart) -> Option<bool> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"xml:space" {
            return Some(attr.value.as_ref() == b"preserve");
        }
    }
    None
}

/// 요소의 지정 속성값(local name 기준)을 읽는다.
fn read_attr_local(e: &BytesStart, local: &[u8]) -> Option<Vec<u8>> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local {
            return Some(attr.value.into_owned());
        }
    }
    None
}

/// 각주/미주의 구분선용 더미 항목인지 판별한다.
/// (LibreOffice writerfilter도 이 세 타입을 건너뛴다)
fn is_dummy_note_type(t: &[u8]) -> bool {
    matches!(
        t,
        b"separator" | b"continuationSeparator" | b"continuationNotice"
    )
}

/// `xml:space="preserve"`가 아닌 `w:t` 텍스트 정규화.
///
/// OOXML 스펙상 preserve가 없으면 가장자리 공백은 무의미하다.
/// LibreOffice `OOXMLFastContextHandler::text()`와 동일하게
/// 앞뒤 XML 공백을 제거하고 내부 탭·개행을 스페이스로 바꾼다.
fn normalize_non_preserved(t: &str) -> Cow<'_, str> {
    let trimmed = t.trim_matches(['\t', '\n', '\r', ' ']);
    if trimmed.contains(['\t', '\n', '\r']) {
        Cow::Owned(trimmed.replace(['\t', '\n', '\r'], " "))
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// WordprocessingML XML에서 텍스트를 추출한다.
///
/// - `<w:t>` → 텍스트 수집, `<w:p>` → 줄바꿈
/// - `<w:br/>`, `<w:cr/>` → 줄바꿈, `<w:tab/>` → 탭
///   (`<w:tabs>` 내부의 탭 정의는 제외)
/// - `<w:tbl>` → 마크다운 테이블 (중첩 표는 바깥 셀 텍스트로 병합)
/// - `<w:instrText>`(필드 코드)와 `<w:delText>`(삭제된 변경 추적 텍스트)는
///   `<w:t>`가 아니므로 자연히 제외되고, `<w:fldChar>` begin~separate
///   구간(필드 커맨드)의 텍스트도 제외한다
/// - 서브트리 스킵: `<mc:Fallback>`(`<mc:Choice>`와 내용 중복),
///   `<w:rt>`(루비 발음), 각주/미주의 separator 더미 항목
fn extract_document_xml(xml: &str, text: &mut String) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    let mut in_t_tag = false;
    let mut t_preserve = false;
    let mut para_has_text = false;
    let mut in_tabs = false; // <w:tabs> (탭 정의) 내부 여부
    let mut skip_depth = 0usize; // 스킵 중인 서브트리 깊이
    let mut preserve_stack: Vec<bool> = Vec::new(); // xml:space 상속
    let mut field_stack: Vec<bool> = Vec::new(); // 필드별 "커맨드 구간" 여부

    // 표 추적: 최상위 표만 마크다운으로 구조화하고, 중첩 표는 셀 텍스트로 흡수
    let mut tbl_depth = 0usize;
    let mut in_tc = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell_text = String::new();
    let mut tc_para_has_text = false;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| HwpError::Docx(format!("XML parse error: {}", e)))?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty = matches!(event, Event::Empty(_));

                if skip_depth > 0 {
                    if !is_empty {
                        skip_depth += 1;
                    }
                    buf.clear();
                    continue;
                }

                let local_name = e.local_name();
                let name = local_name.as_ref();

                // 스킵 대상 서브트리 판별
                let skip = match name {
                    b"Fallback" | b"rt" => true,
                    b"footnote" | b"endnote" => {
                        read_attr_local(e, b"type").is_some_and(|t| is_dummy_note_type(&t))
                    }
                    _ => false,
                };
                if skip {
                    if !is_empty {
                        skip_depth = 1;
                    }
                    buf.clear();
                    continue;
                }

                let preserve = read_xml_space(e)
                    .unwrap_or_else(|| preserve_stack.last().copied().unwrap_or(false));
                if !is_empty {
                    preserve_stack.push(preserve);
                }

                let in_field_command = field_stack.iter().any(|&cmd| cmd);

                match name {
                    b"t" => {
                        in_t_tag = true;
                        t_preserve = preserve;
                    }
                    b"tabs" => in_tabs = true,
                    b"fldChar" => {
                        if let Some(fld_type) = read_attr_local(e, b"fldCharType") {
                            match fld_type.as_slice() {
                                b"begin" => field_stack.push(true),
                                b"separate" => {
                                    if let Some(top) = field_stack.last_mut() {
                                        *top = false;
                                    }
                                }
                                b"end" => {
                                    field_stack.pop();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"tbl" => {
                        tbl_depth += 1;
                        if tbl_depth == 1 {
                            table_rows.clear();
                        }
                    }
                    b"tr" if tbl_depth == 1 => current_row.clear(),
                    b"tc" if tbl_depth == 1 => {
                        in_tc = true;
                        current_cell_text.clear();
                        tc_para_has_text = false;
                    }
                    b"p" => {
                        if in_tc {
                            tc_para_has_text = false;
                        } else {
                            para_has_text = false;
                        }
                    }
                    b"br" | b"cr" if !in_field_command => {
                        if in_tc {
                            current_cell_text.push('\n');
                        } else if tbl_depth == 0 {
                            text.push('\n');
                        }
                    }
                    b"tab" if !in_tabs && !in_field_command => {
                        if in_tc {
                            current_cell_text.push('\t');
                        } else if tbl_depth == 0 {
                            text.push('\t');
                        }
                    }
                    b"noBreakHyphen" if !in_field_command => {
                        if in_tc {
                            current_cell_text.push('\u{2011}');
                            tc_para_has_text = true;
                        } else if tbl_depth == 0 {
                            text.push('\u{2011}');
                            para_has_text = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                    buf.clear();
                    continue;
                }
                preserve_stack.pop();

                let local_name = e.local_name();
                let name = local_name.as_ref();

                match name {
                    b"t" => in_t_tag = false,
                    b"tabs" => in_tabs = false,
                    b"p" => {
                        if in_tc {
                            if tc_para_has_text {
                                current_cell_text.push('\n');
                            }
                        } else if tbl_depth == 0 {
                            if para_has_text {
                                text.push('\n');
                            } else {
                                text.push_str("\n\n");
                            }
                        }
                    }
                    b"tc" if tbl_depth == 1 => {
                        let trimmed = current_cell_text.trim_end_matches('\n').to_string();
                        current_row.push(trimmed);
                        in_tc = false;
                    }
                    b"tr" if tbl_depth == 1 => {
                        if !current_row.is_empty() {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    b"tbl" => {
                        if tbl_depth == 1 {
                            emit_markdown_table(&table_rows, text);
                            table_rows.clear();
                        }
                        tbl_depth = tbl_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Event::Text(ref e) => {
                if in_t_tag && skip_depth == 0 && !field_stack.iter().any(|&cmd| cmd) {
                    let raw = e
                        .unescape()
                        .map_err(|err| HwpError::Docx(format!("XML unescape: {}", err)))?;
                    let t: Cow<'_, str> = if t_preserve {
                        raw
                    } else {
                        Cow::Owned(normalize_non_preserved(&raw).into_owned())
                    };
                    if in_tc {
                        if !t.is_empty() {
                            tc_para_has_text = true;
                        }
                        current_cell_text.push_str(&t);
                    } else if tbl_depth == 0 {
                        if !t.is_empty() {
                            para_has_text = true;
                        }
                        text.push_str(&t);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

/// 표 데이터를 마크다운 테이블로 출력
fn emit_markdown_table(rows: &[Vec<String>], text: &mut String) {
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

/// 마크다운 셀 텍스트 이스케이프: 줄바꿈 → 공백, | → \|
fn escape_markdown_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn test_extract_simple_paragraphs() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<w:document {W_NS}><w:body><w:p><w:r><w:t>안녕하세요</w:t></w:r></w:p><w:p><w:r><w:t>테스트</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text, "안녕하세요\n테스트\n");
    }

    #[test]
    fn test_extract_multi_run() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> World</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn test_xml_space_trim() {
        // preserve가 없으면 가장자리 공백 제거, 있으면 유지
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:t>  A  </w:t></w:r><w:r><w:t xml:space="preserve"> B </w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text, "A B \n");
    }

    #[test]
    fn test_extract_br_and_tab() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>A</w:t><w:br/><w:t>B</w:t><w:tab/><w:t>C</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        // <w:tabs> 내부의 탭 정의는 탭 문자를 만들지 않는다
        assert_eq!(text, "A\nB\tC\n");
    }

    #[test]
    fn test_instr_and_del_text_excluded() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:instrText>PAGE \* MERGEFORMAT</w:instrText></w:r><w:r><w:delText>삭제된 텍스트</w:delText></w:r><w:r><w:t>본문</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text, "본문\n");
    }

    #[test]
    fn test_field_command_excluded_result_kept() {
        // fldChar begin~separate 사이의 w:t는 필드 커맨드이므로 제외,
        // separate~end 사이(필드 결과)는 채택
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:t>커맨드 텍스트</w:t></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>필드 결과</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text, "필드 결과\n");
    }

    #[test]
    fn test_extract_table_markdown() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>셀1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>셀2</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>셀3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>셀4</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert!(text.contains("| 셀1 | 셀2 |"), "got: {text:?}");
        assert!(text.contains("| --- | --- |"), "got: {text:?}");
        assert!(text.contains("| 셀3 | 셀4 |"), "got: {text:?}");
    }

    #[test]
    fn test_nested_table_merged_into_cell() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>바깥</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>안쪽</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        // 중첩 표 텍스트는 바깥 셀 텍스트에 병합된다
        assert!(text.contains("바깥"), "got: {text:?}");
        assert!(text.contains("안쪽"), "got: {text:?}");
        // 마크다운 구분선은 최상위 표 하나만
        assert_eq!(text.matches("---").count(), 1, "got: {text:?}");
    }

    #[test]
    fn test_mc_fallback_not_duplicated() {
        let xml = format!(
            r#"<w:document {W_NS} xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><mc:AlternateContent><mc:Choice Requires="wps"><w:r><w:t>선택 콘텐츠</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>선택 콘텐츠</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text.matches("선택 콘텐츠").count(), 1, "got: {text:?}");
    }

    #[test]
    fn test_footnote_separator_skipped() {
        // separator/continuationSeparator 더미 각주는 제외, 실제 각주는 채택
        let xml = format!(
            r#"<w:footnotes {W_NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:t>더미</w:t></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>실제 각주</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert!(!text.contains("더미"), "got: {text:?}");
        assert!(text.contains("실제 각주"), "got: {text:?}");
    }

    #[test]
    fn test_ruby_rt_excluded() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:ruby><w:rt><w:r><w:t>かんじ</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>漢字</w:t></w:r></w:rubyBase></w:ruby></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert!(text.contains("漢字"), "got: {text:?}");
        assert!(!text.contains("かんじ"), "got: {text:?}");
    }

    #[test]
    fn test_no_break_hyphen() {
        let xml = format!(
            r#"<w:document {W_NS}><w:body><w:p><w:r><w:t>02</w:t><w:noBreakHyphen/><w:t>571</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut text = String::new();
        extract_document_xml(&xml, &mut text).unwrap();
        assert_eq!(text, "02\u{2011}571\n");
    }

    #[test]
    fn test_invalid_xml() {
        let mut text = String::new();
        assert!(extract_document_xml("not xml <<<", &mut text).is_err());
    }

    #[test]
    fn test_nonexistent_file() {
        let path = Path::new("/tmp/nonexistent_docx_12345.docx");
        assert!(extract_text_from_docx(path).is_err());
    }

    #[test]
    fn test_not_a_zip() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_not_a_zip.docx");
        std::fs::write(&path, b"this is not a zip file").unwrap();
        assert!(extract_text_from_docx(&path).is_err());
        std::fs::remove_file(&path).ok();
    }
}
