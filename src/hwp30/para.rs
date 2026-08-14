//! HWP 3.0 문단·컨트롤 파싱.
//!
//! LibreOffice hwpfilter `hpara.cxx` / `hwpread.cxx`의 읽기 순서를 그대로 따라
//! 스트림을 소비하면서 텍스트만 추출한다. 표 셀·글상자·각주·머리말 등의
//! 내장 문단 리스트는 문서 흐름 위치에서 재귀 처리한다.

use crate::error::{HwpError, Result};

use super::cursor::Cursor;
use super::hchar::append_hchar;

/// 컨트롤 문자별 hchar 단위 크기 (`HBox::WSize`). nch 카운트 진행에 사용.
const WSIZE: [usize; 32] = [
    1, 4, 4, 4, 4, 4, 4, 42, //
    48, 4, 4, 4, 4, 1, 4, 4, //
    4, 4, 4, 4, 4, 4, 12, 5, //
    3, 3, 123, 4, 32, 4, 2, 2,
];

const CH_END_PARA: u16 = 13;
/// 내장 문단 리스트 재귀 한도 (손상 파일 방어)
const MAX_DEPTH: usize = 64;

/// 크기 상수 (hinfo.cxx 필드 합산)
const CSHAPE_LEN: usize = 31;
const PSHAPE_LEN: usize = 187;
const LINE_INFO_LEN: usize = 14;
const CELL_LEN: usize = 27;

fn invalid(what: &str) -> HwpError {
    HwpError::Parse(format!("invalid HWP 3.0 structure: {what}"))
}

/// 문단 리스트: nch=0 문단(종결자)이 나올 때까지 반복.
pub fn read_para_list(cur: &mut Cursor, text: &mut String, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(invalid("paragraph nesting too deep"));
    }
    while read_para(cur, text, depth)? {}
    Ok(())
}

/// 문단 하나를 읽는다. 반환값 false = 리스트 종결(nch == 0) 또는 스트림 끝.
fn read_para(cur: &mut Cursor, text: &mut String, depth: usize) -> Result<bool> {
    // 최상위 리스트는 EOF로 끝날 수 있다 (뒤따르는 부가 태그가 없는 경우)
    if cur.is_eof() {
        return Ok(false);
    }
    let reuse_shape = cur.read_u8()?;
    let nch = cur.read_u16()? as usize;
    let nline = cur.read_u16()? as usize;
    let contain_cshape = cur.read_u8()?;
    let _etcflag = cur.read_u8()?;
    let _ctrlflag = cur.read_u32()?;
    let _pstyno = cur.read_u8()?;

    cur.skip(CSHAPE_LEN)?; // 문단 대표 글자모양
    if nch > 0 && reuse_shape == 0 {
        cur.skip(PSHAPE_LEN)?; // 문단모양
    }
    cur.skip(nline * LINE_INFO_LEN)?;

    if contain_cshape != 0 {
        // 글자별 글자모양: 플래그 0이면 CharShape 본체가 뒤따름
        for _ in 0..nch {
            if cur.read_u8()? == 0 {
                cur.skip(CSHAPE_LEN)?;
            }
        }
    }

    if nch == 0 {
        return Ok(false);
    }

    let mut ii = 0usize;
    while ii < nch {
        let hh = cur.read_u16()?;
        if hh == CH_END_PARA {
            break;
        }
        if hh > 31 {
            append_hchar(hh, text);
            ii += 1;
            continue;
        }
        ii += WSIZE[hh as usize];
        read_control(cur, hh, text, depth)?;
    }
    text.push('\n');
    Ok(true)
}

/// 컨트롤 코드(hh)의 뒤따르는 데이터를 소비한다.
///
/// 대부분의 컨트롤은 본체 마지막에 자기 코드를 한 번 더 기록하므로(dummy)
/// 이를 검증해 스트림 어긋남을 조기에 감지한다.
fn read_control(cur: &mut Cursor, hh: u16, text: &mut String, depth: usize) -> Result<()> {
    match hh {
        // 예약 영역: 길이 명시 스킵 블록
        0..=4 | 12 | 27 | 29 => {
            let len = cur.read_u32()? as usize;
            check_dummy(cur, hh)?;
            cur.skip(len)?;
        }
        // 필드 코드(누름틀 등): 표시 텍스트는 일반 문자 스트림에 별도로 존재
        5 => {
            let _size = cur.read_u32()?;
            let _dummy = cur.read_u16()?;
            cur.skip(2 + 4 + 2 + 22)?; // type, reserved1, location_info, reserved2
            let len1 = cur.read_u32()? as usize;
            let len2 = cur.read_u32()? as usize;
            let len3 = cur.read_u32()? as usize;
            let binlen = cur.read_u32()? as usize;
            cur.skip(len1)?;
            cur.skip(len2)?;
            cur.skip(len3)?;
            cur.skip(binlen)?;
        }
        // 책갈피
        6 => {
            let len = cur.read_u32()?;
            check_dummy(cur, hh)?;
            if len != 34 {
                return Err(invalid("bookmark length"));
            }
            cur.skip(34)?;
        }
        // 날짜 서식
        7 => {
            cur.skip(80)?;
            check_dummy(cur, hh)?;
        }
        // 날짜 코드: [년, 월, 요일, 일, 시, 분]
        8 => {
            cur.skip(80)?; // 서식 문자열
            let mut date = [0u16; 6];
            for d in &mut date {
                *d = cur.read_u16()?;
            }
            check_dummy(cur, hh)?;
            if date[0] > 0 {
                text.push_str(&format!("{}년 {}월 {}일", date[0], date[1], date[3]));
            }
        }
        // 탭
        9 => {
            cur.skip(4)?; // width, leader
            check_dummy(cur, hh)?;
            text.push('\t');
        }
        // 표/글상자/수식/버튼 (TxtBox)
        10 => {
            cur.skip(4)?; // reserved
            check_dummy(cur, hh)?;
            cur.skip(78)?; // 배치·크기·여백 등 (hwpread.cxx TxtBox::Read 참조)
            let _type = cur.read_u16()?;
            let ncell = cur.read_u16()? as usize;
            let _protect = cur.read_u16()?;
            if ncell == 0 {
                return Err(invalid("textbox cell count"));
            }
            cur.skip(ncell * CELL_LEN)?;
            for _ in 0..ncell {
                read_para_list(cur, text, depth + 1)?;
            }
            read_para_list(cur, text, depth + 1)?; // 캡션
        }
        // 그림 (내장 그리기 개체 텍스트는 미지원)
        11 => {
            cur.skip(4)?; // reserved
            check_dummy(cur, hh)?;
            let follow = cur.read_u32()? as usize;
            cur.skip(344)?; // 배치 정보 + 그림 경로 등
            cur.skip(follow)?; // 첨부 데이터 (그리기 개체/OLE)
            read_para_list(cur, text, depth + 1)?; // 캡션
        }
        // 선
        14 => {
            cur.skip(4)?;
            check_dummy(cur, hh)?;
            cur.skip(84)?;
        }
        // 숨은 설명
        15 => {
            cur.skip(4)?;
            check_dummy(cur, hh)?;
            cur.skip(8)?; // info
            read_para_list(cur, text, depth + 1)?;
        }
        // 머리말/꼬리말
        16 => {
            cur.skip(4)?;
            check_dummy(cur, hh)?;
            cur.skip(8 + 2)?; // info, type, where
            read_para_list(cur, text, depth + 1)?;
        }
        // 각주/미주
        17 => {
            cur.skip(4)?;
            check_dummy(cur, hh)?;
            cur.skip(8 + 6)?; // info, number, type, width
            read_para_list(cur, text, depth + 1)?;
        }
        // 자동 번호 / 새 번호 / 페이지 번호 표시 / 페이지 번호 제어
        18..=21 => {
            cur.skip(4)?;
            check_dummy(cur, hh)?;
        }
        // 메일 머지
        22 => {
            cur.skip(20)?;
            check_dummy(cur, hh)?;
        }
        // 글자 겹침: 겹쳐진 글자들을 그대로 출력
        23 => {
            let mut compose = [0u16; 3];
            for c in &mut compose {
                *c = cur.read_u16()?;
            }
            check_dummy(cur, hh)?;
            for &c in compose.iter().filter(|&&c| c > 31) {
                append_hchar(c, text);
            }
        }
        // 하이픈
        24 => {
            cur.skip(2)?; // width
            check_dummy(cur, hh)?;
            text.push('-');
        }
        // 차례 표시
        25 => {
            cur.skip(2)?;
            check_dummy(cur, hh)?;
        }
        // 찾아보기 표시
        26 => {
            cur.skip(240 + 2)?; // keyword1, keyword2, pgno
            check_dummy(cur, hh)?;
        }
        // 개요 번호
        28 => {
            cur.skip(60)?;
            check_dummy(cur, hh)?;
        }
        // 묶음 빈칸 / 고정폭 빈칸
        30 | 31 => {
            check_dummy(cur, hh)?;
            text.push(' ');
        }
        _ => unreachable!("hh > 31 and CH_END_PARA handled by caller"),
    }
    Ok(())
}

/// 컨트롤 본체 끝의 반복 코드가 자기 자신과 일치하는지 확인.
fn check_dummy(cur: &mut Cursor, hh: u16) -> Result<()> {
    let dummy = cur.read_u16()?;
    if dummy != hh {
        return Err(invalid("control marker mismatch"));
    }
    Ok(())
}
