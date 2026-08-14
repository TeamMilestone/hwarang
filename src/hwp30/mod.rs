//! HWP 3.0 (한글 3.0~97 바이너리 포맷) 텍스트 추출.
//!
//! LibreOffice hwpfilter의 파싱 로직을 참조해 구현했다.
//! 파일 구조 (hwpfile.cxx `ReadHwpFile` 순서):
//!
//! ```text
//! 시그니처(30B) → 문서정보(128B) → 문서요약(1008B) → 정보블록(가변)
//! → [이하 압축 가능 영역: raw deflate]
//!   글꼴(언어 7종) → 스타일 → 문단 리스트 → 부가 태그
//! ```
//!
//! 알려진 제한: 그리기 개체(follow 블록) 내부의 글상자 텍스트는 추출하지 않는다.

mod cursor;
mod hchar;
mod hnc_table;
mod para;
mod tables;

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::error::{HwpError, Result};
use cursor::Cursor;

/// HWP 3.0 파일 시그니처 (30바이트)
pub const V30_SIGNATURE: &[u8; 30] = b"HWP Document File V3.00 \x1a\x01\x02\x03\x04\x05";
const V2X_PREFIX: &[u8] = b"HWP Document File V2";

/// 문서정보(128B) 내 필드 오프셋 (hinfo.cxx `HWPInfo::Read` 필드 합산)
const OFF_ENCRYPTED: usize = 96;
const OFF_COMPRESSED: usize = 124;
const OFF_INFO_BLOCK_LEN: usize = 126;

/// 문서요약: 제목·지은이 등 9개 필드 × 56 hchars
const SUMMARY_LEN: usize = 9 * 56 * 2;

/// 글꼴 이름 길이, 스타일 항목 길이(이름 20 + 글자모양 31 + 문단모양 187)
const FONTNAME_LEN: usize = 40;
const STYLE_LEN: usize = 20 + 31 + 187;
/// 지원 언어 수 (한글/영문/한자/일어/외국어/기호/사용자)
const NLANGUAGE: usize = 7;

/// HWP 3.0 파일에서 텍스트를 추출한다.
pub fn extract_text_from_hwp30(path: &Path) -> Result<String> {
    let data = fs::read(path)?;
    extract_text(&data)
}

/// 메모리 상의 HWP 3.0 문서에서 텍스트를 추출한다.
pub fn extract_text(data: &[u8]) -> Result<String> {
    let sig = data.get(..30).ok_or(HwpError::UnsupportedFormat)?;
    if sig != V30_SIGNATURE {
        if sig.starts_with(V2X_PREFIX) {
            return Err(HwpError::Parse(
                "HWP 2.x documents are not supported".into(),
            ));
        }
        return Err(HwpError::InvalidSignature);
    }

    let mut cur = Cursor::new(&data[30..]);
    let info = cur.take(128)?;
    let encrypted = u16::from_le_bytes([info[OFF_ENCRYPTED], info[OFF_ENCRYPTED + 1]]);
    if encrypted != 0 {
        return Err(HwpError::PasswordProtected);
    }
    let compressed = info[OFF_COMPRESSED] != 0;
    let info_block_len =
        u16::from_le_bytes([info[OFF_INFO_BLOCK_LEN], info[OFF_INFO_BLOCK_LEN + 1]]) as usize;

    cur.skip(SUMMARY_LEN)?;
    cur.skip(info_block_len)?;

    // 정보블록 이후부터 압축 영역 (raw deflate, zlib 헤더 없음)
    let decompressed;
    let body: &[u8] = if compressed {
        decompressed = inflate_raw(cur.rest())?;
        &decompressed
    } else {
        cur.rest()
    };

    let mut body_cur = Cursor::new(body);
    skip_fonts(&mut body_cur)?;
    skip_styles(&mut body_cur)?;

    let mut text = String::new();
    if let Err(e) = para::read_para_list(&mut body_cur, &mut text, 0) {
        // 비정형/손상 파일: 이미 추출한 텍스트가 있으면 부분 결과를 반환
        if text.trim().is_empty() {
            return Err(e);
        }
    }
    Ok(text)
}

/// raw deflate 스트림 해제. 잘린 스트림은 해제된 부분까지 사용한다.
fn inflate_raw(data: &[u8]) -> Result<Vec<u8>> {
    let mut dec = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match dec.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) => {
                if out.is_empty() {
                    return Err(HwpError::DecompressFailed(e.to_string()));
                }
                break;
            }
        }
    }
    Ok(out)
}

/// 언어별 글꼴 목록 스킵: 언어 7종 × (개수 u16 + 개수 × 40바이트 이름)
fn skip_fonts(cur: &mut Cursor) -> Result<()> {
    for _ in 0..NLANGUAGE {
        let n = cur.read_u16()?;
        if n == 0 || n >= 256 {
            return Err(HwpError::Parse(format!("invalid font count: {n}")));
        }
        cur.skip(n as usize * FONTNAME_LEN)?;
    }
    Ok(())
}

/// 스타일 목록 스킵: 개수 u16 + 개수 × 238바이트
fn skip_styles(cur: &mut Cursor) -> Result<()> {
    let n = cur.read_u16()?;
    if n == 0 || n >= 0x8000 {
        // 원본(hstyle.cxx)은 개수가 0 이하이면 조용히 반환
        return Ok(());
    }
    cur.skip(n as usize * STYLE_LEN)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 최소 HWP 3.0 문서 빌더
    struct Doc {
        body: Vec<u8>,
    }

    impl Doc {
        fn new() -> Self {
            let mut body = Vec::new();
            // 글꼴: 언어 7종 × (개수 1 + 이름 40B)
            for _ in 0..NLANGUAGE {
                body.extend_from_slice(&1u16.to_le_bytes());
                body.extend_from_slice(&[0u8; FONTNAME_LEN]);
            }
            // 스타일 1개
            body.extend_from_slice(&1u16.to_le_bytes());
            body.extend_from_slice(&[0u8; STYLE_LEN]);
            Doc { body }
        }

        /// 문단 추가. `chars`는 CH_END_PARA(13)를 포함한 hchar 시퀀스,
        /// `extra`는 컨트롤 뒤따름 데이터(문자 스트림 밖 페이로드).
        fn para(&mut self, chars: &[u16], extra: &[u8]) -> &mut Self {
            let nch: usize = chars
                .iter()
                .map(|&c| if c < 32 { [1, 4, 4, 4, 4, 4, 4, 42, 48, 4, 4, 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 12, 5, 3, 3, 123, 4, 32, 4, 2, 2][c as usize] } else { 1 })
                .sum();
            self.body.push(0); // reuse_shape
            self.body.extend_from_slice(&(nch as u16).to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // nline
            self.body.extend_from_slice(&[0, 0]); // contain_cshape, etcflag
            self.body.extend_from_slice(&0u32.to_le_bytes()); // ctrlflag
            self.body.push(0); // pstyno
            self.body.extend_from_slice(&[0u8; 31]); // cshape
            self.body.extend_from_slice(&[0u8; 187]); // pshape
            let mut it = chars.iter();
            for &c in it.by_ref() {
                self.body.extend_from_slice(&c.to_le_bytes());
                if c < 32 && c != 13 {
                    break; // 컨트롤 뒤에는 extra 페이로드 그리고 나머지 문자
                }
            }
            self.body.extend_from_slice(extra);
            for &c in it {
                self.body.extend_from_slice(&c.to_le_bytes());
            }
            self
        }

        /// 문단 리스트 종결자 (nch = 0)
        fn end_list(&mut self) -> &mut Self {
            self.body.push(0);
            self.body.extend_from_slice(&0u16.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes());
            self.body.extend_from_slice(&[0, 0]);
            self.body.extend_from_slice(&0u32.to_le_bytes());
            self.body.push(0);
            self.body.extend_from_slice(&[0u8; 31]);
            self
        }

        fn build(&self, compressed: bool) -> Vec<u8> {
            let mut data = Vec::new();
            data.extend_from_slice(V30_SIGNATURE);
            let mut info = [0u8; 128];
            if compressed {
                info[OFF_COMPRESSED] = 1;
            }
            data.extend_from_slice(&info);
            data.extend_from_slice(&[0u8; SUMMARY_LEN]);
            if compressed {
                use std::io::Write;
                let mut enc = flate2::write::DeflateEncoder::new(
                    Vec::new(),
                    flate2::Compression::default(),
                );
                enc.write_all(&self.body).unwrap();
                data.extend_from_slice(&enc.finish().unwrap());
            } else {
                data.extend_from_slice(&self.body);
            }
            data
        }
    }

    /// "가나" = 조합형 하드코딩
    const GA: u16 = 0x8000 | (2 << 10) | (3 << 5) | 1;
    const NA: u16 = 0x8000 | (4 << 10) | (3 << 5) | 1;

    #[test]
    fn test_synthetic_plain_text() {
        let mut doc = Doc::new();
        doc.para(&[u16::from(b'H'), u16::from(b'i'), GA, NA, 13], &[])
            .end_list();
        let text = extract_text(&doc.build(false)).unwrap();
        assert_eq!(text, "Hi가나\n");
    }

    #[test]
    fn test_synthetic_compressed() {
        let mut doc = Doc::new();
        doc.para(&[GA, NA, 13], &[]).end_list();
        let text = extract_text(&doc.build(true)).unwrap();
        assert_eq!(text, "가나\n");
    }

    #[test]
    fn test_synthetic_tab() {
        // 탭(9): width 2B + leader 2B + dummy(=9) 2B
        let mut extra = Vec::new();
        extra.extend_from_slice(&[0u8; 4]);
        extra.extend_from_slice(&9u16.to_le_bytes());
        let mut doc = Doc::new();
        doc.para(&[u16::from(b'a'), 9, u16::from(b'b'), 13], &extra)
            .end_list();
        let text = extract_text(&doc.build(false)).unwrap();
        assert_eq!(text, "a\tb\n");
    }

    #[test]
    fn test_synthetic_table_cells() {
        // 표(10): reserved 4B, dummy 2B, 고정 78B, type/ncell/protect 6B,
        // 셀 2×27B, 셀 문단리스트 2개, 캡션 문단리스트
        let mut extra = Vec::new();
        extra.extend_from_slice(&[0u8; 4]); // reserved
        extra.extend_from_slice(&10u16.to_le_bytes()); // dummy
        extra.extend_from_slice(&[0u8; 78]);
        extra.extend_from_slice(&0u16.to_le_bytes()); // type = table
        extra.extend_from_slice(&2u16.to_le_bytes()); // ncell
        extra.extend_from_slice(&0u16.to_le_bytes()); // protect
        extra.extend_from_slice(&[0u8; 2 * 27]); // cells

        // 셀 1: "A", 셀 2: "B", 캡션: 빈 리스트
        let mut cell = Doc { body: Vec::new() };
        cell.para(&[u16::from(b'A'), 13], &[]).end_list();
        cell.para(&[u16::from(b'B'), 13], &[]).end_list();
        cell.end_list(); // 캡션
        extra.extend_from_slice(&cell.body);

        let mut doc = Doc::new();
        doc.para(&[u16::from(b'x'), 10, u16::from(b'y'), 13], &extra)
            .end_list();
        let text = extract_text(&doc.build(false)).unwrap();
        assert_eq!(text, "xA\nB\ny\n");
    }

    #[test]
    fn test_signature_mismatch() {
        let result = extract_text(b"not an hwp 3.0 file, definitely..");
        assert!(matches!(result, Err(HwpError::InvalidSignature)));
    }

    #[test]
    fn test_too_short() {
        let result = extract_text(b"HWP");
        assert!(matches!(result, Err(HwpError::UnsupportedFormat)));
    }

    #[test]
    fn test_v2_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(b"HWP Document File V2.00 \x1a\x01\x02\x03\x04\x05");
        data.extend_from_slice(&[0u8; 200]);
        let result = extract_text(&data);
        assert!(matches!(result, Err(HwpError::Parse(_))));
    }

    #[test]
    fn test_encrypted_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(V30_SIGNATURE);
        let mut info = [0u8; 128];
        info[OFF_ENCRYPTED] = 1;
        data.extend_from_slice(&info);
        let result = extract_text(&data);
        assert!(matches!(result, Err(HwpError::PasswordProtected)));
    }
}
