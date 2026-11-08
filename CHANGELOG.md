# Changelog

이 프로젝트의 주요 변경 사항을 기록합니다.

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르며,
버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 준수합니다.

## [0.5.1] - 2026-11-08

### Changed

- **투명 표 줄의 칸 사이를 탭으로** — 0.5.0 은 칸을 빈칸으로 이어 항목 이름 칸과 내용 칸의
  경계가 사라졌다("계약담당자 한국농수산대학교 …"). 이제 칸 사이에 탭을 넣어 읽는 쪽이
  열을 다시 맞출 수 있다("계약담당자\t한국농수산대학교 …")
  - 줄 머리 기호만 든 칸("○", "1", "1.", "가.", "(가)", "①")은 전처럼 빈칸으로 잇는다
  - 앞 칸이 여러 줄이면 다음 칸은 새 줄에서, 뒤 칸이 여러 줄이면 첫 줄만 탭 뒤에 붙는다
  - 빈칸·전각 빈칸만 든 칸은 버린다
  - 공고문 120개: 낱말·순서는 그대로, 바뀐 35개에서 탭으로 나뉜 줄 125 → 303

## [0.5.0] - 2026-11-08

### Changed

- **목록 배치용 투명 표는 표가 아니라 본문 줄로 출력** — HWP·HWPX·HWPML
  - 공고문 작성자가 번호·글머리표 들여쓰기를 맞추려고 테두리 없는 표를 자주 쓴다.
    전에는 모두 마크다운 표가 돼 본문이 빈 칸 많은 표로 보였다
  - 칸의 테두리 정의를 읽는다: HWPX `header.xml` 의 `hh:borderFill`(칸의
    `borderFillIDRef`), HWPML `BORDERFILL`(칸의 `BorderFill`), HWP 5.0 문서 정보의
    BORDER_FILL 레코드(칸 머리의 테두리/배경 번호, 변마다 종류 0 = 없음)
  - 이웃한 칸 사이 세로선이 하나도 보이지 않는 줄(투명 표, 바깥 테두리만 있는 상자)은
    채운 칸을 이어 한 줄로 쓰고, 칸 사이 세로선이 있는 줄이 이어진 부분만 표로 쓴다
  - 테두리 정의를 못 읽거나 그런 줄이 없는 표는 전과 같다
  - 예: 공고문 한 건(HWPX) 표 줄 221 → 5, 낱말 수 그대로

## [0.4.1] - 2026-11-08

### Fixed

- **HWPX·HWPML 표 안의 표에서 바깥 셀 글이 사라지던 문제**
  - 표 상태를 하나만 들고 있어, 안쪽 표가 시작될 때 바깥 셀에 모은 글을 지우고
    안쪽 표가 끝난 뒤의 바깥 표 행도 버렸다. 문서 전체를 표로 감싼 공고문은
    본문이 통째로 빠졌다 (예: 본문 14,521자 → 추출 2,550바이트)
  - 열린 표를 스택으로 쌓고, 끝난 안쪽 표는 감싼 셀 글에 넣는다
    (HWP 바이너리·DOCX 와 같은 방식 — 여러 칸 표 안에서는 한 줄로 이어진다)
  - 빈 `<hp:t/>` 가 글 수집을 켠 채로 두던 것, 빈 `<hp:tc/>` 가 칸을 연 채로
    두던 것을 바로잡았다

### Changed

- **1×1 표(테두리 상자)는 마크다운 표 대신 본문 문단으로 출력** — HWP·HWPX·HWPML·DOCX
  - 한 칸짜리 표는 칸 안 줄바꿈을 공백으로 바꿔 상자 전체가 한 줄이 됐다
    (공고문을 1×1 표로 감싸면 수천 자가 한 줄). 이제 줄을 살려 그대로 쓴다

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
