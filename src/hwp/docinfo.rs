use std::collections::HashMap;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{HwpError, Result};
use crate::hwp::record::{self, Record};

/// DocInfo에서 필요한 최소 정보
#[derive(Debug)]
pub struct DocInfo {
    pub section_count: u16,
    /// 테두리/배경 번호(1부터) → 변마다 선이 보이는가 (0.5.0)
    pub border_fills: HashMap<u16, crate::hwpx::Sides>,
}

/// BORDER_FILL 레코드: 속성 UINT16 뒤에 왼쪽·오른쪽·위·아래 테두리가 [종류 UINT8, 굵기 UINT8, 색 COLORREF] 로
/// 이어진다. 종류 0 이 "없음"(1 실선, 2 긴 점선 …)
fn border_sides(data: &[u8]) -> Option<crate::hwpx::Sides> {
    (data.len() >= 26).then(|| crate::hwpx::Sides {
        left: data[2] != 0,
        right: data[8] != 0,
        top: data[14] != 0,
        bottom: data[20] != 0,
    })
}

/// DocInfo 레코드 시퀀스에서 section_count를 추출한다.
/// DOCUMENT_PROPERTIES (첫 번째 레코드)의 첫 u16이 section_count.
pub fn parse_doc_info(records: &[Record]) -> Result<DocInfo> {
    let first = records
        .first()
        .ok_or_else(|| HwpError::Parse("Empty DocInfo records".into()))?;

    if first.header.tag_id != record::HWPTAG_DOCUMENT_PROPERTIES {
        return Err(HwpError::Parse(format!(
            "Expected DOCUMENT_PROPERTIES, got tag 0x{:X}",
            first.header.tag_id
        )));
    }

    if first.data.len() < 2 {
        return Err(HwpError::Parse("DOCUMENT_PROPERTIES too short".into()));
    }

    let section_count = (&first.data[..2]).read_u16::<LittleEndian>()?;

    let border_fills = records
        .iter()
        .filter(|r| r.header.tag_id == record::HWPTAG_BORDER_FILL)
        .enumerate()
        .filter_map(|(i, r)| Some(((i + 1) as u16, border_sides(&r.data)?)))
        .collect();

    Ok(DocInfo {
        section_count,
        border_fills,
    })
}

#[cfg(test)]
mod border_tests {
    use super::*;

    #[test]
    fn reads_border_sides() {
        // 실제 공고문(구미전자정보기술원) 문서 정보의 BORDER_FILL 두 개: 네 변 없음 / 네 변 실선 0.5mm
        let none = vec![0u8; 40];
        let mut solid = vec![0u8; 40];
        for o in [2usize, 8, 14, 20] {
            solid[o] = 1;
            solid[o + 1] = 5;
        }
        assert_eq!(border_sides(&none), Some(crate::hwpx::Sides::default()));
        let all = crate::hwpx::Sides {
            left: true,
            right: true,
            top: true,
            bottom: true,
        };
        assert_eq!(border_sides(&solid), Some(all));
        assert_eq!(border_sides(&[0u8; 10]), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_section_count() {
        // section_count=1, 나머지 24바이트는 0
        let mut data = vec![0u8; 26];
        data[0] = 1; // section_count = 1 (LE)
        data[1] = 0;

        let records = vec![Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_DOCUMENT_PROPERTIES,
                level: 0,
                size: 26,
            },
            data,
        }];

        let info = parse_doc_info(&records).unwrap();
        assert_eq!(info.section_count, 1);
    }

    #[test]
    fn test_parse_section_count_multi() {
        let mut data = vec![0u8; 26];
        data[0] = 3; // section_count = 3
        data[1] = 0;

        let records = vec![Record {
            header: record::RecordHeader {
                tag_id: record::HWPTAG_DOCUMENT_PROPERTIES,
                level: 0,
                size: 26,
            },
            data,
        }];

        let info = parse_doc_info(&records).unwrap();
        assert_eq!(info.section_count, 3);
    }
}
