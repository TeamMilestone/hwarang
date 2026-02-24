/// 문자 타입
#[derive(Debug, PartialEq)]
pub enum CharType {
    Normal,
    ControlChar,
    ControlInline,
    ControlExtend,
}

/// 코드값으로 문자 타입 판별
#[inline(always)]
pub fn char_type(code: u16) -> CharType {
    if code > 31 {
        return CharType::Normal;
    }
    match code {
        // ControlExtend (16 bytes): 그리기/표/수식/필드/머리글/각주 등
        1 | 2 | 3 | 11 | 12 | 14 | 15 | 16 | 17 | 18 | 21 | 22 | 23 => CharType::ControlExtend,
        // ControlInline (16 bytes): 탭, 필드끝 등
        4 | 5 | 6 | 7 | 8 | 9 | 19 | 20 => CharType::ControlInline,
        // ControlChar (2 bytes): 줄바꿈, 문단끝, 하이픈 등
        _ => CharType::ControlChar,
    }
}

/// ControlExtend 코드 중 텍스트를 포함하는 컨트롤 (CTRL_HEADER가 뒤따름)
/// code 11: 그리기 개체/표/수식
/// code 15: 숨은 설명
/// code 16: 머리글/꼬리글
/// code 17: 각주/미주
pub fn is_text_control(code: u16) -> bool {
    matches!(code, 11 | 15 | 16 | 17)
}

/// 텍스트 세그먼트: ControlExtend 위치에서 분할된 텍스트 조각
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    /// 이 세그먼트 뒤에 ControlExtend가 있는지
    pub has_control_after: bool,
}

/// PARA_TEXT 레코드 데이터를 모든 ControlExtend 위치에서 분할하여 세그먼트 목록을 반환한다.
///
/// 모든 ControlExtend에서 분할하여, 대응하는 CTRL_HEADER 서브트리와 1:1 매칭할 수 있게 한다.
/// 텍스트가 없는 컨트롤(구역정의 등)의 서브트리는 재귀 시 자연스럽게 빈 출력을 생성한다.
pub fn extract_text_segments(data: &[u8]) -> Vec<TextSegment> {
    let len = data.len();
    let mut segments = Vec::new();
    let mut current = String::with_capacity(len / 2);
    let mut pos = 0;

    while pos + 1 < len {
        let code = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        match char_type(code) {
            CharType::Normal => {
                if let Some(ch) = char::from_u32(code as u32) {
                    current.push(ch);
                }
            }
            CharType::ControlChar => match code {
                10 => current.push('\n'),
                13 => {}
                24 => current.push('-'),
                30 => current.push(' '),
                31 => current.push(' '),
                _ => {}
            },
            CharType::ControlInline => {
                let skip = 14.min(len - pos);
                pos += skip;
                if code == 9 {
                    current.push('\t');
                }
            }
            CharType::ControlExtend => {
                let skip = 14.min(len - pos);
                pos += skip;

                // 모든 ControlExtend에서 분할
                segments.push(TextSegment {
                    text: std::mem::take(&mut current),
                    has_control_after: true,
                });
            }
        }
    }

    // 마지막 세그먼트
    segments.push(TextSegment {
        text: current,
        has_control_after: false,
    });

    segments
}

/// PARA_TEXT 레코드 데이터에서 텍스트를 추출한다.
/// 반환: (추출된 텍스트, ControlExtend 코드 목록)
///
/// 직접 바이트 슬라이스 접근으로 Cursor/ReadBytesExt 오버헤드 제거
pub fn extract_text(data: &[u8]) -> (String, Vec<u16>) {
    let len = data.len();
    // 대략적 용량 사전할당: UTF-16 코드유닛 수의 절반 정도
    let mut text = String::with_capacity(len / 2);
    let mut controls = Vec::new();
    let mut pos = 0;

    while pos + 1 < len {
        let code = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        match char_type(code) {
            CharType::Normal => {
                // UTF-16LE 단일 코드 유닛 → char (BMP)
                if let Some(ch) = char::from_u32(code as u32) {
                    text.push(ch);
                }
            }
            CharType::ControlChar => {
                match code {
                    10 => text.push('\n'), // 줄바꿈
                    13 => {}               // 문단 끝 (무시)
                    24 => text.push('-'),  // 하이픈
                    30 => text.push(' '),  // 묶음 빈칸
                    31 => text.push(' '),  // 고정폭 빈칸
                    _ => {}
                }
            }
            CharType::ControlInline => {
                // 14바이트 스킵
                let skip = 14.min(len - pos);
                pos += skip;

                if code == 9 {
                    text.push('\t'); // 탭
                }
            }
            CharType::ControlExtend => {
                controls.push(code);
                // 14바이트 스킵
                let skip = 14.min(len - pos);
                pos += skip;
            }
        }
    }

    (text, controls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_type_normal() {
        assert_eq!(char_type(32), CharType::Normal); // space
        assert_eq!(char_type(65), CharType::Normal); // 'A'
        assert_eq!(char_type(0xAC00), CharType::Normal); // '가'
    }

    #[test]
    fn test_char_type_control() {
        assert_eq!(char_type(10), CharType::ControlChar); // line break
        assert_eq!(char_type(13), CharType::ControlChar); // para break
        assert_eq!(char_type(9), CharType::ControlInline); // tab
        assert_eq!(char_type(11), CharType::ControlExtend); // drawing/table
        assert_eq!(char_type(15), CharType::ControlExtend); // hidden comment
        assert_eq!(char_type(16), CharType::ControlExtend); // header/footer
        assert_eq!(char_type(17), CharType::ControlExtend); // footnote/endnote
    }

    #[test]
    fn test_extract_simple_text() {
        // "AB" in UTF-16LE
        let data = vec![0x41, 0x00, 0x42, 0x00];
        let (text, controls) = extract_text(&data);
        assert_eq!(text, "AB");
        assert!(controls.is_empty());
    }

    #[test]
    fn test_extract_korean_text() {
        // "가나" in UTF-16LE: 0xAC00, 0xB098
        let data = vec![0x00, 0xAC, 0x98, 0xB0];
        let (text, _) = extract_text(&data);
        assert_eq!(text, "가나");
    }

    #[test]
    fn test_extract_with_line_break() {
        // "A\nB" - A(0x41), LF(0x0A), B(0x42)
        let data = vec![0x41, 0x00, 0x0A, 0x00, 0x42, 0x00];
        let (text, _) = extract_text(&data);
        assert_eq!(text, "A\nB");
    }

    #[test]
    fn test_extract_with_tab() {
        // "A\tB" - A(0x41), TAB(0x09 + 14bytes), B(0x42)
        let mut data = vec![0x41, 0x00]; // A
        data.extend_from_slice(&[0x09, 0x00]); // tab code
        data.extend_from_slice(&[0u8; 14]); // tab addition
        data.extend_from_slice(&[0x42, 0x00]); // B
        let (text, _) = extract_text(&data);
        assert_eq!(text, "A\tB");
    }

    #[test]
    fn test_extract_with_control_extend() {
        // "A" + control_extend(11) + "B"
        let mut data = vec![0x41, 0x00]; // A
        data.extend_from_slice(&[0x0B, 0x00]); // code 11 (table/drawing)
        data.extend_from_slice(&[0u8; 14]); // addition
        data.extend_from_slice(&[0x42, 0x00]); // B
        let (text, controls) = extract_text(&data);
        assert_eq!(text, "AB");
        assert_eq!(controls, vec![11]);
    }

    #[test]
    fn test_extract_segments_no_control() {
        // "AB" - 컨트롤 없음
        let data = vec![0x41, 0x00, 0x42, 0x00];
        let segments = extract_text_segments(&data);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "AB");
        assert!(!segments[0].has_control_after);
    }

    #[test]
    fn test_extract_segments_with_table() {
        // "A" + control_extend(11=table) + "B"
        let mut data = vec![0x41, 0x00]; // A
        data.extend_from_slice(&[0x0B, 0x00]); // code 11 (table/drawing)
        data.extend_from_slice(&[0u8; 14]); // addition
        data.extend_from_slice(&[0x42, 0x00]); // B
        let segments = extract_text_segments(&data);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "A");
        assert!(segments[0].has_control_after);
        assert_eq!(segments[1].text, "B");
        assert!(!segments[1].has_control_after);
    }

    #[test]
    fn test_extract_segments_multiple_controls() {
        // "A" + table(11) + "B" + footnote(17) + "C"
        let mut data = vec![0x41, 0x00]; // A
        data.extend_from_slice(&[0x0B, 0x00]); // table
        data.extend_from_slice(&[0u8; 14]);
        data.extend_from_slice(&[0x42, 0x00]); // B
        data.extend_from_slice(&[0x11, 0x00]); // footnote (17)
        data.extend_from_slice(&[0u8; 14]);
        data.extend_from_slice(&[0x43, 0x00]); // C
        let segments = extract_text_segments(&data);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "A");
        assert!(segments[0].has_control_after);
        assert_eq!(segments[1].text, "B");
        assert!(segments[1].has_control_after);
        assert_eq!(segments[2].text, "C");
        assert!(!segments[2].has_control_after);
    }

    #[test]
    fn test_extract_segments_all_control_extend_splits() {
        // "A" + control_extend(1) + "B"
        // 모든 ControlExtend에서 분할
        let mut data = vec![0x41, 0x00]; // A
        data.extend_from_slice(&[0x01, 0x00]); // code 1
        data.extend_from_slice(&[0u8; 14]);
        data.extend_from_slice(&[0x42, 0x00]); // B
        let segments = extract_text_segments(&data);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "A");
        assert!(segments[0].has_control_after);
        assert_eq!(segments[1].text, "B");
        assert!(!segments[1].has_control_after);
    }
}
