//! 실물 HWP 3.0 문서 통합 테스트.
//!
//! 픽스처: 기업마당(BIZINFO) 공개 공고문 「2026 사업화 패키지 지원 (방산분야)
//! 모집공고」를 한글 97(HWP 3.0) 포맷으로 저장한 파일.
//! 기대 텍스트는 같은 폴더의 PDF 변환본(2026사업화패키지지원.pdf)과 대조해 검증했다.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_bizinfo_hwp30_extraction() {
    let path = fixture("BIZINFO_PBLN_000000000124226_main_hwp30.hwp");
    let text = hwarang::extract_text_from_file(&path).unwrap();

    // 제목과 본문
    assert!(text.contains("2026 사업화 패키지 지원"));
    assert!(text.contains("(방산분야) 모집공고"));
    assert!(text.contains("성남시 방위산업 생태계 활성화 및 기업 경쟁력 강화"));
    assert!(text.contains("협약체결일 ~ 2026. 10. 30."));

    // 표 셀 텍스트 (지원 진행절차)
    assert!(text.contains("모집 및 선정"));
    assert!(text.contains("최종심사 및 정산"));
    assert!(text.contains("10월 ~ 11월"));

    // HNC 특수기호 변환 (LibreOffice hwpfilter는 □로 뭉개는 영역)
    assert!(text.contains("○ 단독형"));
    assert!(text.contains("• 진흥원 홈페이지"));
    assert!(text.contains("※ 지원 기업으로"));
    assert!(text.contains("☞ (NICE 신용정보 조회 시)"));
    assert!(text.contains("수집·열람·활용"));
    assert!(text.contains('⇨'));
    assert!(text.contains("“회수의문” “휴폐업” “부도”"));

    // 문서 끝
    assert!(text.contains("혁신성장부 박기언 차장 (031-750-2921)"));
    assert!(text.trim_end().ends_with("성남산업진흥원장"));

    // 변환 실패 문자가 없어야 함
    assert!(
        !text.contains('\u{25A1}') || text.contains("□ 사업목적"),
        "본문 불릿(□) 외의 미매핑 문자가 없어야 한다"
    );
}

#[test]
fn test_bizinfo_hwp30_no_garbage() {
    let path = fixture("BIZINFO_PBLN_000000000124226_main_hwp30.hwp");
    let text = hwarang::extract_text_from_file(&path).unwrap();

    // 제어 문자·PUA가 새어나오지 않아야 함
    assert!(!text.chars().any(|c| {
        (c.is_control() && c != '\n' && c != '\t') || ('\u{E000}'..='\u{F8FF}').contains(&c)
    }));
}
