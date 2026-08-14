# Changelog

이 프로젝트의 주요 변경 사항을 기록합니다.

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며,
버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 준수합니다.

## [0.4.0] - 2026-08-14

### Added

- **DOCX(OOXML WordprocessingML) 텍스트 추출 지원**
  - ZIP 매직 바이트 공유 문제 해결: `PK` 시그니처 파일은 아카이브 내용으로
    판별 (`word/document.xml` 존재 → DOCX, 그 외 → HWPX)
  - `w:t` 텍스트, `w:br`/`w:cr` 줄바꿈, `w:tab` 탭, `w:noBreakHyphen` 추출
  - `xml:space="preserve"` 미지정 시 가장자리 공백 정규화
    (LibreOffice writerfilter 동작 참조)
  - 표(`w:tbl`)를 마크다운 테이블로 변환, 중첩 표는 바깥 셀 텍스트로 병합
  - 필드 커맨드(`w:fldChar` begin~separate, `w:instrText`), 삭제된 변경
    추적 텍스트(`w:delText`), `mc:Fallback` 중복 콘텐츠, 루비 발음(`w:rt`)
    제외
  - 각주/미주(`word/footnotes.xml`/`endnotes.xml`) 추출 — separator 더미
    항목 제외
  - CLI 디렉토리 모드에서 `.docx` 확장자 수집
- 실물 DOCX 문서 픽스처(기업마당 공고문) 및 통합 테스트

### Notes

- DOCX 처리 속도: 100KB급 공고문 기준 파일당 약 0.6ms (단일 스레드),
  배치 병렬 처리 시 약 4,000 files/s (Apple Silicon 8코어)

## [0.3.0] - 2026-08-14

### Added

- **HWP 3.0(한글 3.0~97 바이너리 포맷) 텍스트 추출 지원**
  - `HWP Document File V3.00` 시그니처 자동 감지 — 기존
    `extract_text_from_file` API 그대로 사용
  - 문서정보/요약/정보블록 파싱, raw deflate 압축 스트림 해제
  - 문단·컨트롤(표 셀, 글상자, 각주/미주, 머리말/꼬리말, 숨은설명 등)
    재귀 파싱 (LibreOffice hwpfilter 참조)
  - 한글 조합형(KSSM) 산술 변환 + 한컴 공식 HNC 테이블 기반
    특수문자·한자 변환
  - LibreOffice hwpfilter 대비 정확도 개선: 0x3400 미만 특수기호
    (○ • ※ ☞ ⇨ · 등)와 KS X 1001 외 한자까지 정확 변환
- 실물 HWP 3.0 문서 픽스처(기업마당 공고문) 및 통합 테스트

### Notes

- HWP 2.x 문서는 시그니처를 감지해 명확한 오류를 반환 (변환 미지원)
- 그리기 개체(follow 블록) 내부의 글상자 텍스트는 추출하지 않음

## [0.2.0] - 2026-02-27

### Added

- 라이브러리 병렬 처리 API: `extract_text_batch` (rayon 파일 수준 +
  HWP 섹션 수준 병렬)

## [0.1.0] - 2026-02-24

### Added

- HWP 5.x(OLE 바이너리) 텍스트 추출: FileHeader/DocInfo/BodyText 파싱,
  zlib 압축 해제, 컨트롤 디스패치(표, 글상자, 각주, EQEDIT 수식 등)
- 배포문서(읽기 전용 DRM) 복호화 (LCG XOR + AES/ECB)
- HWPX(ZIP/OWPML)·HWPML(순수 XML) 텍스트 추출
- 매직 바이트 기반 포맷 자동 감지
- rayon 병렬 배치 CLI (`hwarang <입력> -o <출력>`)

[0.3.0]: https://github.com/teammilestone/hwarang/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/teammilestone/hwarang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/teammilestone/hwarang/releases/tag/v0.1.0
