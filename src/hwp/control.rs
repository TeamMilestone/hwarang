use byteorder::{LittleEndian, ReadBytesExt};

/// 컨트롤 ID 상수 (4바이트, big-endian ASCII)
pub const CTRL_TABLE: u32 = make_ctrl_id(b"tbl ");
pub const CTRL_GSO: u32 = make_ctrl_id(b"gso ");
pub const CTRL_EQUATION: u32 = make_ctrl_id(b"eqed");
pub const CTRL_HEADER: u32 = make_ctrl_id(b"head");
pub const CTRL_FOOTER: u32 = make_ctrl_id(b"foot");
pub const CTRL_FOOTNOTE: u32 = make_ctrl_id(b"fn  ");
pub const CTRL_ENDNOTE: u32 = make_ctrl_id(b"en  ");
pub const CTRL_HIDDEN_COMMENT: u32 = make_ctrl_id(b"tcmt");
pub const CTRL_FORM: u32 = make_ctrl_id(b"form");

/// 4바이트 ASCII → u32 (big-endian)
const fn make_ctrl_id(id: &[u8; 4]) -> u32 {
    ((id[0] as u32) << 24) | ((id[1] as u32) << 16) | ((id[2] as u32) << 8) | (id[3] as u32)
}

/// 레코드 데이터에서 컨트롤 ID를 읽는다 (CTRL_HEADER 레코드의 첫 4바이트)
pub fn read_ctrl_id(data: &[u8]) -> Option<u32> {
    if data.len() < 4 {
        return None;
    }
    (&data[..4]).read_u32::<LittleEndian>().ok()
}

/// 컨트롤 ID → 이름 (디버그용)
pub fn ctrl_name(id: u32) -> &'static str {
    match id {
        CTRL_TABLE => "Table",
        CTRL_GSO => "Gso",
        CTRL_EQUATION => "Equation",
        CTRL_HEADER => "Header",
        CTRL_FOOTER => "Footer",
        CTRL_FOOTNOTE => "Footnote",
        CTRL_ENDNOTE => "Endnote",
        CTRL_HIDDEN_COMMENT => "HiddenComment",
        CTRL_FORM => "Form",
        _ => "Unknown",
    }
}

/// 텍스트를 포함하는 컨트롤인지 확인
pub fn has_paragraph_list(id: u32) -> bool {
    matches!(
        id,
        CTRL_TABLE
            | CTRL_GSO
            | CTRL_HEADER
            | CTRL_FOOTER
            | CTRL_FOOTNOTE
            | CTRL_ENDNOTE
            | CTRL_HIDDEN_COMMENT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_id_make() {
        assert_eq!(CTRL_TABLE, make_ctrl_id(b"tbl "));
        // 't' = 0x74, 'b' = 0x62, 'l' = 0x6C, ' ' = 0x20
        assert_eq!(CTRL_TABLE, 0x74626C20);
    }

    #[test]
    fn test_read_ctrl_id() {
        // HWP 바이너리는 리틀엔디안: "tbl " = 0x74626C20 → LE bytes [0x20, 0x6C, 0x62, 0x74]
        let data = [0x20, 0x6C, 0x62, 0x74];
        assert_eq!(read_ctrl_id(&data), Some(CTRL_TABLE));
    }
}
