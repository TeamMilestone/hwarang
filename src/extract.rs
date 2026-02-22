use byteorder::{LittleEndian, ReadBytesExt};

use crate::hwp::para_text;
use crate::hwp::record::{self, Record};

/// 섹션 레코드 시퀀스에서 텍스트를 추출한다.
///
/// 단일패스 방식: 레코드를 순차 탐색하면서
/// - PARA_TEXT → 텍스트 추출
/// - EQEDIT → 수식 스크립트 텍스트 추출
///
/// 모든 PARA_TEXT 레코드가 순서대로 나타나므로, level에 관계없이 모두 추출하면
/// 본문 + 표 + 머리글/꼬리글 + 각주/미주 + 숨은설명의 텍스트가 올바른 순서로 나온다.
pub fn extract_section_text(records: &[Record], text: &mut String) {
    for rec in records {
        match rec.header.tag_id {
            record::HWPTAG_PARA_TEXT => {
                let (para_text, _controls) = para_text::extract_text(&rec.data);
                if !para_text.is_empty() {
                    text.push_str(&para_text);
                }
                text.push('\n');
            }
            record::HWPTAG_EQEDIT => {
                if let Some(script) = extract_eqedit_script(&rec.data) {
                    if !script.is_empty() {
                        text.push_str(&script);
                        text.push('\n');
                    }
                }
            }
            _ => {}
        }
    }
}

/// EQEDIT 레코드에서 수식 스크립트 텍스트를 추출한다.
/// 레이아웃: property(4B) + script(HWPString)
/// HWPString: length(u16 LE, 문자 수) + data(length*2 bytes, UTF-16LE)
fn extract_eqedit_script(data: &[u8]) -> Option<String> {
    if data.len() < 6 {
        return None; // 최소 property(4) + length(2)
    }

    let mut cursor = std::io::Cursor::new(&data[4..]); // property 스킵
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

    // UTF-16LE 디코딩
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
        // property(4B) + HWPString("AB" = 2 chars)
        let mut data = vec![0u8; 4]; // property
        data.extend_from_slice(&2u16.to_le_bytes()); // char_count = 2
        data.extend_from_slice(&[0x41, 0x00, 0x42, 0x00]); // "AB" UTF-16LE

        let script = extract_eqedit_script(&data).unwrap();
        assert_eq!(script, "AB");
    }

    #[test]
    fn test_extract_eqedit_empty() {
        let mut data = vec![0u8; 4]; // property
        data.extend_from_slice(&0u16.to_le_bytes()); // char_count = 0

        let script = extract_eqedit_script(&data).unwrap();
        assert_eq!(script, "");
    }

    #[test]
    fn test_extract_section_text_simple() {
        // PARA_HEADER + PARA_TEXT("Hello")
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
                    level: 0,
                    size: 10,
                },
                // "Hello" in UTF-16LE
                data: vec![0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F, 0x00],
            },
        ];

        let mut text = String::new();
        extract_section_text(&records, &mut text);
        assert_eq!(text, "Hello\n");
    }
}
