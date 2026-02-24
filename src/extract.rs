use byteorder::{LittleEndian, ReadBytesExt};

use crate::hwp::control;
use crate::hwp::para_text;
use crate::hwp::record::{self, Record};

/// 섹션 레코드 시퀀스에서 텍스트를 추출한다.
///
/// 커서 기반 재귀 방식: PARA_TEXT를 ControlExtend 위치에서 분할하고,
/// 컨트롤 서브트리(표 셀, 각주, 텍스트박스 등)를 인라인으로 재귀 처리하여
/// 문서 흐름 순서대로 텍스트를 출력한다.
pub fn extract_section_text(records: &[Record], text: &mut String) {
    let mut pos = 0;
    extract_para_list(records, &mut pos, 0, text);
}

/// 주어진 base_level의 PARA_HEADER 시퀀스를 처리한다.
fn extract_para_list(records: &[Record], pos: &mut usize, base_level: u16, text: &mut String) {
    while *pos < records.len() {
        let rec = &records[*pos];
        if rec.header.level < base_level {
            break;
        }
        if rec.header.tag_id == record::HWPTAG_PARA_HEADER && rec.header.level == base_level {
            extract_para(records, pos, base_level, text);
        } else {
            *pos += 1;
        }
    }
}

/// 단일 문단 추출: PARA_TEXT 세그먼트 + 컨트롤 인라인 재귀
///
/// HWP 레코드 레벨 구조:
///   PARA_HEADER level=L
///     PARA_TEXT level=L+1
///     PARA_CHAR_SHAPE level=L+1
///     CTRL_HEADER level=L+1
///       TABLE level=L+2
///       LIST_HEADER level=L+2
///       PARA_HEADER level=L+2 (셀 내부)
fn extract_para(records: &[Record], pos: &mut usize, level: u16, text: &mut String) {
    // PARA_HEADER 스킵
    *pos += 1;

    let para_start = *pos;
    let child_level = level + 1; // PARA_TEXT, CTRL_HEADER 등의 레벨

    let mut para_text_data: Option<&[u8]> = None;
    // 모든 CTRL_HEADER 서브트리 (ControlExtend 순서와 1:1 대응)
    let mut all_ctrl_subtrees: Vec<(usize, usize)> = Vec::new();
    let mut eqedit_texts: Vec<String> = Vec::new();

    // 문단 범위 스캔
    let mut scan = para_start;
    while scan < records.len() {
        let rec = &records[scan];

        // 같은 level의 PARA_HEADER → 다음 문단
        if rec.header.tag_id == record::HWPTAG_PARA_HEADER && rec.header.level == level {
            break;
        }
        // level보다 낮은 레벨 → 상위 복귀
        if rec.header.level < level {
            break;
        }

        if rec.header.tag_id == record::HWPTAG_PARA_TEXT && rec.header.level == child_level {
            para_text_data = Some(&rec.data);
        } else if rec.header.tag_id == record::HWPTAG_CTRL_HEADER && rec.header.level == child_level
        {
            // CTRL_HEADER 서브트리 범위 기록
            let ctrl_start = scan;
            let ctrl_level = rec.header.level;
            scan += 1;
            // 서브트리: ctrl_level보다 깊은 레코드들
            while scan < records.len() && records[scan].header.level > ctrl_level {
                scan += 1;
            }
            all_ctrl_subtrees.push((ctrl_start, scan));
            continue;
        } else if rec.header.tag_id == record::HWPTAG_EQEDIT && rec.header.level > level {
            if let Some(script) = extract_eqedit_script(&rec.data) {
                if !script.is_empty() {
                    eqedit_texts.push(script);
                }
            }
        }

        scan += 1;
    }

    *pos = scan;

    // PARA_TEXT가 없으면 빈 문단
    let Some(pt_data) = para_text_data else {
        text.push_str("\n\n");
        return;
    };

    // 세그먼트 분할 (모든 ControlExtend에서 분할 → CTRL_HEADER와 1:1 대응)
    let segments = para_text::extract_text_segments(pt_data);

    // 교차 출력: segment[0] → ctrl_subtree[0] → segment[1] → ctrl_subtree[1] → ...
    let mut ctrl_idx = 0;
    for seg in &segments {
        if !seg.text.is_empty() {
            text.push_str(&seg.text);
        }
        if seg.has_control_after && ctrl_idx < all_ctrl_subtrees.len() {
            let (sub_start, sub_end) = all_ctrl_subtrees[ctrl_idx];
            extract_ctrl_subtree(records, sub_start, sub_end, text);
            ctrl_idx += 1;
        }
    }

    // 남은 ctrl_subtrees 처리
    while ctrl_idx < all_ctrl_subtrees.len() {
        let (sub_start, sub_end) = all_ctrl_subtrees[ctrl_idx];
        extract_ctrl_subtree(records, sub_start, sub_end, text);
        ctrl_idx += 1;
    }

    // 수식 텍스트 출력
    for eq in &eqedit_texts {
        text.push_str(eq);
        text.push('\n');
    }

    text.push('\n');
}

/// 컨트롤 서브트리 내의 텍스트 추출 (표 셀, 각주, 텍스트박스 등)
fn extract_ctrl_subtree(records: &[Record], start: usize, end: usize, text: &mut String) {
    // 표 컨트롤이면 마크다운 테이블로 출력
    if let Some(ctrl_id) = control::read_ctrl_id(&records[start].data) {
        if ctrl_id == control::CTRL_TABLE {
            extract_table_subtree(records, start, end, text);
            return;
        }
    }

    let mut i = start + 1; // CTRL_HEADER 스킵

    while i < end {
        let rec = &records[i];
        if rec.header.tag_id == record::HWPTAG_LIST_HEADER {
            i += 1;
            // LIST_HEADER 다음에 PARA_HEADER가 오면 재귀 처리
            if i < end && records[i].header.tag_id == record::HWPTAG_PARA_HEADER {
                let para_level = records[i].header.level;
                extract_para_list_bounded(records, &mut i, para_level, end, text);
            }
        } else if rec.header.tag_id == record::HWPTAG_EQEDIT {
            if let Some(script) = extract_eqedit_script(&rec.data) {
                if !script.is_empty() {
                    text.push_str(&script);
                    text.push('\n');
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
}

/// TABLE 레코드에서 행/열 수를 파싱한다.
fn parse_table_dimensions(data: &[u8]) -> Option<(u16, u16)> {
    if data.len() < 8 {
        return None;
    }
    let rows = u16::from_le_bytes([data[4], data[5]]);
    let cols = u16::from_le_bytes([data[6], data[7]]);
    Some((rows, cols))
}

/// LIST_HEADER 레코드에서 셀 위치(col, row, colSpan, rowSpan)를 파싱한다.
fn parse_cell_position(data: &[u8]) -> Option<(u16, u16, u16, u16)> {
    if data.len() < 16 {
        return None;
    }
    let col = u16::from_le_bytes([data[8], data[9]]);
    let row = u16::from_le_bytes([data[10], data[11]]);
    let col_span = u16::from_le_bytes([data[12], data[13]]);
    let row_span = u16::from_le_bytes([data[14], data[15]]);
    Some((col, row, col_span, row_span))
}

/// 마크다운 셀 텍스트 이스케이프: 줄바꿈 → 공백, | → \|
fn escape_markdown_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// 셀 데이터를 마크다운 테이블 문자열로 포맷한다.
fn format_markdown_table(cells: &[(u16, u16, String)], rows: u16, cols: u16) -> String {
    // 2D grid 구성
    let rows = rows as usize;
    let cols = cols as usize;
    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); cols]; rows];

    for (col, row, content) in cells {
        let r = *row as usize;
        let c = *col as usize;
        if r < rows && c < cols {
            grid[r][c] = content.clone();
        }
    }

    let mut result = String::new();
    for (i, row) in grid.iter().enumerate() {
        result.push('|');
        for cell in row {
            let escaped = escape_markdown_cell(cell.trim_end_matches('\n'));
            result.push(' ');
            result.push_str(&escaped);
            result.push_str(" |");
        }
        result.push('\n');

        // 첫 행 뒤에 구분선
        if i == 0 {
            result.push('|');
            for _ in 0..cols {
                result.push_str(" --- |");
            }
            result.push('\n');
        }
    }

    result
}

/// 표 컨트롤 서브트리에서 마크다운 테이블을 추출한다.
fn extract_table_subtree(records: &[Record], start: usize, end: usize, text: &mut String) {
    let mut i = start + 1; // CTRL_HEADER 스킵

    // TABLE 레코드 찾기
    let mut rows: u16 = 0;
    let mut cols: u16 = 0;
    let mut found_table = false;

    while i < end {
        if records[i].header.tag_id == record::HWPTAG_TABLE {
            if let Some((r, c)) = parse_table_dimensions(&records[i].data) {
                rows = r;
                cols = c;
                found_table = true;
            }
            i += 1;
            break;
        }
        i += 1;
    }

    if !found_table || rows == 0 || cols == 0 {
        // fallback: 기존 선형 출력
        extract_ctrl_subtree_linear(records, start, end, text);
        return;
    }

    // LIST_HEADER 위치를 모두 수집하여 셀 범위를 결정
    let list_header_level = if i < end && records[i].header.tag_id == record::HWPTAG_LIST_HEADER {
        records[i].header.level
    } else {
        extract_ctrl_subtree_linear(records, start, end, text);
        return;
    };

    let mut cell_ranges: Vec<(usize, usize)> = Vec::new(); // (list_header_idx, cell_end_idx)
    let mut list_header_indices: Vec<usize> = Vec::new();

    // TABLE 이후의 LIST_HEADER들을 수집
    let mut j = i;
    while j < end {
        if records[j].header.tag_id == record::HWPTAG_LIST_HEADER
            && records[j].header.level == list_header_level
        {
            list_header_indices.push(j);
        }
        j += 1;
    }

    // 각 LIST_HEADER의 셀 범위 결정: 현재 LIST_HEADER ~ 다음 LIST_HEADER (또는 end)
    for (idx, &lh_idx) in list_header_indices.iter().enumerate() {
        let cell_end = if idx + 1 < list_header_indices.len() {
            list_header_indices[idx + 1]
        } else {
            end
        };
        cell_ranges.push((lh_idx, cell_end));
    }

    // 각 셀에서 텍스트 추출
    let mut cells: Vec<(u16, u16, String)> = Vec::new();

    for (lh_idx, cell_end) in &cell_ranges {
        let cell_pos = parse_cell_position(&records[*lh_idx].data);
        let mut cell_text = String::new();

        // LIST_HEADER 다음 레코드부터 셀 범위까지 추출
        let mut ci = *lh_idx + 1;
        if ci < *cell_end && records[ci].header.tag_id == record::HWPTAG_PARA_HEADER {
            let para_level = records[ci].header.level;
            extract_para_list_bounded(records, &mut ci, para_level, *cell_end, &mut cell_text);
        }

        if let Some((col, row, _, _)) = cell_pos {
            cells.push((col, row, cell_text));
        } else {
            let idx = cells.len() as u16;
            let row_idx = if cols > 0 { idx / cols } else { 0 };
            let col_idx = if cols > 0 { idx % cols } else { 0 };
            cells.push((col_idx, row_idx, cell_text));
        }
    }

    let table_str = format_markdown_table(&cells, rows, cols);
    text.push_str(&table_str);
}

/// 표가 아닌 컨트롤의 선형 텍스트 추출 (fallback)
fn extract_ctrl_subtree_linear(records: &[Record], start: usize, end: usize, text: &mut String) {
    let mut i = start + 1;
    while i < end {
        let rec = &records[i];
        if rec.header.tag_id == record::HWPTAG_LIST_HEADER {
            i += 1;
            if i < end && records[i].header.tag_id == record::HWPTAG_PARA_HEADER {
                let para_level = records[i].header.level;
                extract_para_list_bounded(records, &mut i, para_level, end, text);
            }
        } else if rec.header.tag_id == record::HWPTAG_EQEDIT {
            if let Some(script) = extract_eqedit_script(&rec.data) {
                if !script.is_empty() {
                    text.push_str(&script);
                    text.push('\n');
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
}

/// extract_para_list의 bounded 버전: end 인덱스까지만 처리
fn extract_para_list_bounded(
    records: &[Record],
    pos: &mut usize,
    base_level: u16,
    end: usize,
    text: &mut String,
) {
    while *pos < end {
        let rec = &records[*pos];
        if rec.header.level < base_level {
            break;
        }
        if rec.header.tag_id == record::HWPTAG_PARA_HEADER && rec.header.level == base_level {
            extract_para(records, pos, base_level, text);
            if *pos > end {
                *pos = end;
            }
        } else {
            *pos += 1;
        }
    }
}

/// EQEDIT 레코드에서 수식 스크립트 텍스트를 추출한다.
fn extract_eqedit_script(data: &[u8]) -> Option<String> {
    if data.len() < 6 {
        return None;
    }

    let mut cursor = std::io::Cursor::new(&data[4..]);
    let char_count = cursor.read_u16::<LittleEndian>().ok()? as usize;

    if char_count == 0 {
        return Some(String::new());
    }

    let remaining = data.len() - 6;
    let byte_count = char_count * 2;
    if remaining < byte_count {
        return None;
    }

    let start = 6;
    let utf16_data = &data[start..start + byte_count];

    let utf16_units: Vec<u16> = utf16_data
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    String::from_utf16(&utf16_units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_eqedit_script() {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&[0x41, 0x00, 0x42, 0x00]);

        let script = extract_eqedit_script(&data).unwrap();
        assert_eq!(script, "AB");
    }

    #[test]
    fn test_extract_eqedit_empty() {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&0u16.to_le_bytes());

        let script = extract_eqedit_script(&data).unwrap();
        assert_eq!(script, "");
    }

    #[test]
    fn test_extract_section_text_simple() {
        // PARA_HEADER level=0 + PARA_TEXT level=1 ("Hello")
        let records = vec![
            Record {
                header: record::RecordHeader {
                    tag_id: record::HWPTAG_PARA_HEADER,
                    level: 0,
                    size: 0,
                },
                data: vec![],
            },
            Record {
                header: record::RecordHeader {
                    tag_id: record::HWPTAG_PARA_TEXT,
                    level: 1, // child of PARA_HEADER
                    size: 10,
                },
                data: vec![0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F, 0x00],
            },
        ];

        let mut text = String::new();
        extract_section_text(&records, &mut text);
        assert_eq!(text, "Hello\n");
    }

    #[test]
    fn test_extract_section_text_with_table_inline() {
        // 실제 HWP 레벨 구조 반영:
        // PARA_HEADER level=0
        //   PARA_TEXT level=1: "A [TABLE_CTRL] B"
        //   CTRL_HEADER level=1 (표)
        //     LIST_HEADER level=2
        //     PARA_HEADER level=2
        //       PARA_TEXT level=3: "셀1"
        let mut records = vec![];

        // PARA_HEADER level=0
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_PARA_HEADER,
                level: 0,
                size: 0,
            },
            data: vec![],
        });

        // PARA_TEXT level=1: "A" + ControlExtend(11=table) + "B"
        let mut pt_data = vec![0x41, 0x00]; // A
        pt_data.extend_from_slice(&[0x0B, 0x00]); // code 11 (table)
        pt_data.extend_from_slice(&[0u8; 14]); // addition
        pt_data.extend_from_slice(&[0x42, 0x00]); // B
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_PARA_TEXT,
                level: 1,
                size: pt_data.len() as u32,
            },
            data: pt_data,
        });

        // CTRL_HEADER level=1 (표 컨트롤)
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_CTRL_HEADER,
                level: 1,
                size: 0,
            },
            data: vec![],
        });

        // LIST_HEADER level=2
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_LIST_HEADER,
                level: 2,
                size: 0,
            },
            data: vec![],
        });

        // PARA_HEADER level=2 (셀 내부)
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_PARA_HEADER,
                level: 2,
                size: 0,
            },
            data: vec![],
        });

        // PARA_TEXT level=3: "셀1"
        let cell_data: Vec<u8> = "셀1".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        records.push(Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_PARA_TEXT,
                level: 3,
                size: cell_data.len() as u32,
            },
            data: cell_data,
        });

        let mut text = String::new();
        extract_section_text(&records, &mut text);

        // A → 셀1 → B 순서
        let a_pos = text.find('A').expect("Should contain 'A'");
        let cell_pos = text.find("셀1").expect("Should contain '셀1'");
        let b_pos = text.find('B').expect("Should contain 'B'");
        assert!(a_pos < cell_pos, "A should come before 셀1");
        assert!(cell_pos < b_pos, "셀1 should come before B");
    }

    #[test]
    fn test_escape_markdown_cell_pipe() {
        assert_eq!(escape_markdown_cell("a|b"), "a\\|b");
    }

    #[test]
    fn test_escape_markdown_cell_newline() {
        assert_eq!(escape_markdown_cell("line1\nline2"), "line1 line2");
    }

    #[test]
    fn test_escape_markdown_cell_empty() {
        assert_eq!(escape_markdown_cell(""), "");
    }

    #[test]
    fn test_escape_markdown_cell_combined() {
        assert_eq!(escape_markdown_cell("a|b\nc"), "a\\|b c");
    }

    #[test]
    fn test_format_markdown_table_basic() {
        let cells = vec![
            (0u16, 0u16, "A".to_string()),
            (1, 0, "B".to_string()),
            (0, 1, "C".to_string()),
            (1, 1, "D".to_string()),
        ];
        let table = format_markdown_table(&cells, 2, 2);
        assert!(table.contains("| A |"));
        assert!(table.contains("| --- |"));
        assert!(table.contains("| C |"));
    }

    #[test]
    fn test_eqedit_script_too_short() {
        let data = vec![0u8; 3]; // 6바이트 미만
        assert!(extract_eqedit_script(&data).is_none());
    }

    #[test]
    fn test_eqedit_script_insufficient_body() {
        // char_count=10이지만 실제 데이터가 부족
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&10u16.to_le_bytes());
        // 20바이트 필요하지만 0바이트만 있음
        assert!(extract_eqedit_script(&data).is_none());
    }

    #[test]
    fn test_extract_section_text_empty_records() {
        let records: Vec<Record> = vec![];
        let mut text = String::new();
        extract_section_text(&records, &mut text);
        assert!(text.is_empty());
    }

    #[test]
    fn test_extract_section_text_para_header_only() {
        // PARA_HEADER만 있고 PARA_TEXT가 없는 경우
        let records = vec![Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_PARA_HEADER,
                level: 0,
                size: 0,
            },
            data: vec![],
        }];
        let mut text = String::new();
        extract_section_text(&records, &mut text);
        // 빈 문단 → "\n\n"
        assert_eq!(text, "\n\n");
    }
}
