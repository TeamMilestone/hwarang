//! 실물 DOCX 문서 통합 테스트.
//!
//! 픽스처: 기업마당(BIZINFO) 공개 공고문 「2026년 서울시 소상공인 온라인 유통
//! MD 상담 참여 소상공인 모집 공고」 DOCX 원본.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_bizinfo_docx_extraction() {
    let path = fixture("BIZINFO_PBLN_000000000124107_main.docx");
    let text = hwarang::extract_text_from_file(&path).unwrap();

    // 제목과 본문
    assert!(text.contains("서울특별시 공고 제 2026-2048호"));
    assert!(text.contains("2026년 서울시 소상공인 온라인 유통 MD 상담"));
    assert!(text.contains("참여 소상공인 모집 공고"));
    assert!(text.contains("(모집기간) 2026. 7. 7. (화) ~ 11. 13 (금)까지 상시모집"));

    // 표 셀 텍스트 (지원 제외 사항)
    assert!(text.contains("휴·폐업 및 부도"));
    assert!(text.contains("국세·지방세 체납, 금융기관 등의 채무불이행이 확인된 경우"));

    // 표 셀 텍스트 (구비서류)
    assert!(text.contains("사업자등록증 또는 사업자등록증명원"));
    assert!(text.contains("중소기업(소상공인) 확인서"));

    // 마크다운 테이블로 구조화되었는지
    assert!(
        text.contains("| --- |"),
        "표가 마크다운으로 출력되어야 한다"
    );
    assert!(text.contains("| 세금 체납 | 국세·지방세 체납"));
    assert!(text.contains("| 구분 | 구비서류 | 개수 | 비고 |"));

    // 문서 끝 (문의처)
    assert!(text.contains("WK마케팅그룹: 02-571-7752"));
    assert!(text.contains("서울시: 02-2133-5143 / peachf61@seoul.go.kr"));
}

#[test]
fn test_bizinfo_docx_no_garbage() {
    let path = fixture("BIZINFO_PBLN_000000000124107_main.docx");
    let text = hwarang::extract_text_from_file(&path).unwrap();

    // 제어 문자·PUA가 새어나오지 않아야 함
    assert!(!text.chars().any(|c| {
        (c.is_control() && c != '\n' && c != '\t') || ('\u{E000}'..='\u{F8FF}').contains(&c)
    }));

    // XML 태그·속성이 새어나오지 않아야 함
    assert!(!text.contains("w:rsid"));
    assert!(!text.contains("</"));
}
