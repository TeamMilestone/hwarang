use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{HwpError, Result};
use crate::hwp::record::{self, Record};

/// DocInfo에서 필요한 최소 정보
#[derive(Debug)]
pub struct DocInfo {
    pub section_count: u16,
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

    Ok(DocInfo { section_count })
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
