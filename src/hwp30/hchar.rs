//! HWP 3.0 2바이트 문자(hchar) → 유니코드 변환.
//!
//! 한글 조합형(0x8000~)은 LibreOffice hwpfilter `hcode.cxx`의
//! `kssm_hangul_to_ucs2` 산술 조합을 이식했고, 그 외 영역(한자·특수문자·불릿)은
//! 한컴 공식 HNC → 유니코드 테이블(`hnc_table.rs`)을 직접 조회한다.
//! hwpfilter는 0x3400 미만 특수문자를 □로 뭉개고 한자를 심벌 테이블로 잘못
//! 매핑하는 반면, HNC 테이블은 전 영역을 정확히 변환한다.

use super::hnc_table::HNC_TO_UNICODE_LO;
use super::tables::{
    CHOSEONG_TO_UNICODE, JAMOCOMP1_TO_UNICODE, JAMO_TO_UNICODE, JONGSEONG_TO_UNICODE,
    JOONGSEONG_TO_UNICODE,
};

const NUM_JOONGSEONG: u16 = 21;
const NUM_JONGSEONG: u16 = 28;
const UNI_HANGUL_FIRST: u16 = 0xac00;
const LINEBASE: u16 = 0x3013;
/// 매핑 불가 문자 표시 (hwpfilter와 동일하게 흰 네모)
const UNMAPPED: char = '\u{25A1}';

/// hchar 하나를 유니코드로 변환해 `out`에 추가한다.
pub fn append_hchar(ch: u16, out: &mut String) {
    if ch < 128 {
        out.push(ch as u8 as char);
        return;
    }
    if ch & 0x8000 != 0 {
        // "한"의 상용 조합 코드는 단독 예외 처리 (hcode.cxx 동일)
        if ch == 0xd3c5 {
            out.push('한');
            return;
        }
        append_kssm_hangul(ch, out);
        return;
    }
    if let Some(c) = line_char(ch) {
        return out.push(c);
    }
    // "한글과컴퓨터" 전용 글자: HNC 테이블에서도 PUA라 직접 매핑
    if (0x37c0..=0x37c5).contains(&ch) {
        return out.push(match ch {
            0x37c0 => '한',
            0x37c1 => '글',
            0x37c2 => '과',
            0x37c3 => '컴',
            0x37c4 => '퓨',
            _ => '터',
        });
    }
    match HNC_TO_UNICODE_LO[(ch - 0x80) as usize] {
        0 => out.push(UNMAPPED),
        v => push_u16(v, out),
    }
}

fn push_u16(v: u16, out: &mut String) {
    match char::from_u32(v as u32) {
        Some(c) if v != 0 => out.push(c),
        _ => {}
    }
}

/// 조합형 한글: 초성 5비트 / 중성 5비트 / 종성 5비트 (`kssm_hangul_to_ucs2` 이식)
fn append_kssm_hangul(ch: u16, out: &mut String) {
    let cho = (ch >> 10) & 0x1f;
    let jung = (ch >> 5) & 0x1f;
    let jong = ch & 0x1f;

    if jung < 2 {
        // 비조합 영역: 자모 단독 및 옛한글 조합
        if jung == 0 && ch < 0xa414 {
            let idx = (cho * 32 + jong) as usize;
            match JAMO_TO_UNICODE.get(idx) {
                Some(&v) => push_u16(v, out),
                None => out.push(UNMAPPED),
            }
        } else {
            let idx = (cho * 32 + jong).checked_sub(308).map(usize::from);
            match idx.and_then(|i| JAMOCOMP1_TO_UNICODE.get(i)) {
                Some(&(size, v1, v2, v3)) => {
                    push_u16(v1, out);
                    if size >= 2 {
                        push_u16(v2, out);
                    }
                    if size >= 3 {
                        push_u16(v3, out);
                    }
                }
                None => out.push(UNMAPPED),
            }
        }
        return;
    }
    if cho == 1 && jong == 1 {
        // 모음 단독
        push_u16(JOONGSEONG_TO_UNICODE[jung as usize], out);
        return;
    }
    if jung == 2 && jong == 1 {
        // 자음 단독
        push_u16(CHOSEONG_TO_UNICODE[cho as usize], out);
        return;
    }
    // 불완전 음절: 개별 자모로 분해 출력
    if cho > 20
        || cho == 0
        || jung == 17
        || jung == 24
        || jung == 25
        || jung > 29
        || jong == 0
        || jong == 18
        || jong > 29
        || cho == 1
        || jung == 2
    {
        if cho != 1 {
            push_u16(CHOSEONG_TO_UNICODE[cho as usize], out);
        }
        if jung > 2 {
            push_u16(JOONGSEONG_TO_UNICODE[jung as usize], out);
        }
        if jong != 1 {
            push_u16(JONGSEONG_TO_UNICODE[jong as usize], out);
        }
        return;
    }

    // 완성형 음절 산술 조합 (가..힣)
    let cho = cho - 2;
    let jung = jung
        - if jung < 0x8 {
            3
        } else if jung < 0x10 {
            5
        } else if jung < 0x18 {
            7
        } else {
            9
        };
    let jong = jong - if jong > 0x12 { 2 } else { 1 };
    push_u16(
        UNI_HANGUL_FIRST + cho * NUM_JOONGSEONG * NUM_JONGSEONG + jung * NUM_JONGSEONG + jong,
        out,
    );
}

/// 표 그리기 선문자 영역(0x3013~0x305F)을 -, |, + 로 근사
fn line_char(ch: u16) -> Option<char> {
    const INDEX2DIR: [u8; 11] = [10, 11, 9, 14, 15, 13, 6, 7, 5, 3, 12];
    if !(LINEBASE..LINEBASE + 11 * 7).contains(&ch) {
        return None;
    }
    Some(match INDEX2DIR[((ch - LINEBASE) % 11) as usize] {
        3 => '-',
        12 => '|',
        _ => '+',
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(ch: u16) -> String {
        let mut s = String::new();
        append_hchar(ch, &mut s);
        s
    }

    #[test]
    fn test_ascii() {
        assert_eq!(conv(b'A' as u16), "A");
        assert_eq!(conv(b' ' as u16), " ");
    }

    #[test]
    fn test_hangul_syllables() {
        // 조합형 "가" = 초성 ㄱ(2) 중성 ㅏ(3) 종성 없음(1) → 0x8000|2<<10|3<<5|1
        assert_eq!(conv(0x8000 | (2 << 10) | (3 << 5) | 1), "가");
        // "한" 상용 코드
        assert_eq!(conv(0xd3c5), "한");
        // "힣" = ㅎ(20) ㅣ(29) ㅎ(29)
        assert_eq!(conv(0x8000 | (20 << 10) | (29 << 5) | 29), "힣");
    }

    #[test]
    fn test_jamo() {
        // 자음 단독 ㄱ: 초성 2, 중성 2, 종성 1 → 첫가끝 초성 U+1100 (hwpfilter 동일)
        assert_eq!(conv(0x8000 | (2 << 10) | (2 << 5) | 1), "\u{1100}");
        // 모음 단독 ㅏ: 초성 1, 중성 3, 종성 1 → 첫가끝 중성 U+1161
        assert_eq!(conv(0x8000 | (1 << 10) | (3 << 5) | 1), "\u{1161}");
    }

    #[test]
    fn test_hanja() {
        // 첫 한자 0x4000 → KS 0xCAA1 → 伽
        assert_eq!(conv(0x4000), "伽");
    }

    #[test]
    fn test_line_chars() {
        assert_eq!(conv(LINEBASE + 9), "-");
        assert_eq!(conv(LINEBASE + 10), "|");
        assert_eq!(conv(LINEBASE), "+");
    }

    #[test]
    fn test_hancom_chars() {
        assert_eq!(conv(0x37c0), "한");
        assert_eq!(conv(0x37c5), "터");
    }
}
