use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::error::{HwpError, Result};

/// HWP 태그 상수 (BEGIN = 0x10)
pub const HWPTAG_BEGIN: u16 = 0x10;

// DocInfo 태그
pub const HWPTAG_DOCUMENT_PROPERTIES: u16 = HWPTAG_BEGIN;

// BodyText 태그
pub const HWPTAG_PARA_HEADER: u16 = HWPTAG_BEGIN + 50;
pub const HWPTAG_PARA_TEXT: u16 = HWPTAG_BEGIN + 51;
pub const HWPTAG_PARA_CHAR_SHAPE: u16 = HWPTAG_BEGIN + 52;
pub const HWPTAG_PARA_LINE_SEG: u16 = HWPTAG_BEGIN + 53;
pub const HWPTAG_PARA_RANGE_TAG: u16 = HWPTAG_BEGIN + 54;
pub const HWPTAG_CTRL_HEADER: u16 = HWPTAG_BEGIN + 55;
pub const HWPTAG_LIST_HEADER: u16 = HWPTAG_BEGIN + 56;
pub const HWPTAG_PAGE_DEF: u16 = HWPTAG_BEGIN + 57;
pub const HWPTAG_FOOTNOTE_SHAPE: u16 = HWPTAG_BEGIN + 58;
pub const HWPTAG_PAGE_BORDER_FILL: u16 = HWPTAG_BEGIN + 59;
pub const HWPTAG_SHAPE_COMPONENT: u16 = HWPTAG_BEGIN + 60;
pub const HWPTAG_TABLE: u16 = HWPTAG_BEGIN + 61;
pub const HWPTAG_SHAPE_COMPONENT_LINE: u16 = HWPTAG_BEGIN + 62;
pub const HWPTAG_SHAPE_COMPONENT_RECTANGLE: u16 = HWPTAG_BEGIN + 63;
pub const HWPTAG_SHAPE_COMPONENT_ELLIPSE: u16 = HWPTAG_BEGIN + 64;
pub const HWPTAG_SHAPE_COMPONENT_ARC: u16 = HWPTAG_BEGIN + 65;
pub const HWPTAG_SHAPE_COMPONENT_POLYGON: u16 = HWPTAG_BEGIN + 66;
pub const HWPTAG_SHAPE_COMPONENT_CURVE: u16 = HWPTAG_BEGIN + 67;
pub const HWPTAG_SHAPE_COMPONENT_OLE: u16 = HWPTAG_BEGIN + 68;
pub const HWPTAG_SHAPE_COMPONENT_PICTURE: u16 = HWPTAG_BEGIN + 69;
pub const HWPTAG_SHAPE_COMPONENT_CONTAINER: u16 = HWPTAG_BEGIN + 70;
pub const HWPTAG_CTRL_DATA: u16 = HWPTAG_BEGIN + 71;
pub const HWPTAG_EQEDIT: u16 = HWPTAG_BEGIN + 72;
pub const HWPTAG_SHAPE_COMPONENT_TEXTART: u16 = HWPTAG_BEGIN + 74;
pub const HWPTAG_FORM_OBJECT: u16 = HWPTAG_BEGIN + 75;
pub const HWPTAG_MEMO_SHAPE: u16 = HWPTAG_BEGIN + 76;
pub const HWPTAG_MEMO_LIST: u16 = HWPTAG_BEGIN + 77;
pub const HWPTAG_FORBIDDEN_CHAR: u16 = HWPTAG_BEGIN + 78;
pub const HWPTAG_CHART_DATA: u16 = HWPTAG_BEGIN + 79;

/// 레코드 헤더
/// 4바이트 packed: tag(10bit) | level(10bit) | size(12bit)
/// size == 4095이면 추가 4바이트로 실제 크기
#[derive(Debug, Clone)]
pub struct RecordHeader {
    pub tag_id: u16,
    pub level: u16,
    pub size: u32,
}

impl RecordHeader {
    /// 태그 이름 (디버그용)
    pub fn tag_name(&self) -> &'static str {
        match self.tag_id {
            HWPTAG_DOCUMENT_PROPERTIES => "DOCUMENT_PROPERTIES",
            HWPTAG_PARA_HEADER => "PARA_HEADER",
            HWPTAG_PARA_TEXT => "PARA_TEXT",
            HWPTAG_PARA_CHAR_SHAPE => "PARA_CHAR_SHAPE",
            HWPTAG_PARA_LINE_SEG => "PARA_LINE_SEG",
            HWPTAG_PARA_RANGE_TAG => "PARA_RANGE_TAG",
            HWPTAG_CTRL_HEADER => "CTRL_HEADER",
            HWPTAG_LIST_HEADER => "LIST_HEADER",
            HWPTAG_PAGE_DEF => "PAGE_DEF",
            HWPTAG_FOOTNOTE_SHAPE => "FOOTNOTE_SHAPE",
            HWPTAG_PAGE_BORDER_FILL => "PAGE_BORDER_FILL",
            HWPTAG_SHAPE_COMPONENT => "SHAPE_COMPONENT",
            HWPTAG_TABLE => "TABLE",
            HWPTAG_CTRL_DATA => "CTRL_DATA",
            HWPTAG_EQEDIT => "EQEDIT",
            _ => "UNKNOWN",
        }
    }
}

/// 레코드 = 헤더 + 바디
#[derive(Debug)]
pub struct Record {
    pub header: RecordHeader,
    pub data: Vec<u8>,
}

/// 바이트 슬라이스에서 레코드 시퀀스를 파싱한다.
pub fn read_records(data: &[u8]) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    let mut cursor = std::io::Cursor::new(data);

    while (cursor.position() as usize) < data.len() {
        // 최소 4바이트 필요
        if data.len() - (cursor.position() as usize) < 4 {
            break;
        }

        let value = cursor.read_u32::<LittleEndian>()?;
        let tag_id = (value & 0x3FF) as u16; // bits 0-9
        let level = ((value >> 10) & 0x3FF) as u16; // bits 10-19
        let mut size = (value >> 20) & 0xFFF; // bits 20-31

        // 확장 크기: size == 4095이면 추가 4바이트
        if size == 4095 {
            if data.len() - (cursor.position() as usize) < 4 {
                return Err(HwpError::InvalidRecordHeader);
            }
            size = cursor.read_u32::<LittleEndian>()?;
        }

        let header = RecordHeader {
            tag_id,
            level,
            size,
        };

        // 레코드 바디 읽기
        let pos = cursor.position() as usize;
        let end = pos + size as usize;
        if end > data.len() {
            return Err(HwpError::Parse(format!(
                "Record body overflow: need {} bytes at pos {}, but only {} available",
                size,
                pos,
                data.len() - pos
            )));
        }

        let mut body = vec![0u8; size as usize];
        cursor.read_exact(&mut body)?;

        records.push(Record { header, data: body });
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_header_parse() {
        // tag=0x10 (16), level=0, size=26
        // packed: (26 << 20) | (0 << 10) | 16 = 0x01A00010
        let value: u32 = (26 << 20) | (0 << 10) | 16;
        let bytes = value.to_le_bytes();
        // Add 26 bytes of body
        let mut data = Vec::from(&bytes[..]);
        data.extend_from_slice(&vec![0u8; 26]);

        let records = read_records(&data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].header.tag_id, HWPTAG_DOCUMENT_PROPERTIES);
        assert_eq!(records[0].header.level, 0);
        assert_eq!(records[0].header.size, 26);
        assert_eq!(records[0].data.len(), 26);
    }

    #[test]
    fn test_record_header_extended_size() {
        // tag=HWPTAG_PARA_TEXT(0x43), level=1, size=4095 (extended)
        let value: u32 = (4095 << 20) | (1 << 10) | (HWPTAG_PARA_TEXT as u32);
        let mut data = Vec::from(&value.to_le_bytes()[..]);
        // Extended size: 5000
        data.extend_from_slice(&5000u32.to_le_bytes());
        // Body: 5000 bytes
        data.extend_from_slice(&vec![0u8; 5000]);

        let records = read_records(&data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].header.tag_id, HWPTAG_PARA_TEXT);
        assert_eq!(records[0].header.level, 1);
        assert_eq!(records[0].header.size, 5000);
    }
}
