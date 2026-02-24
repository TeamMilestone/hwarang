# hwarang

HWP/HWPX 문서에서 텍스트를 빠르게 추출하는 Rust crate입니다.

## 배경

한국에서 가장 널리 쓰이는 문서 형식인 HWP(한글과컴퓨터 한/글)는 독자적인 바이너리 포맷을 사용합니다. 대량의 HWP 문서에서 텍스트를 빠르게 추출해야 하는 필요가 있었고, 읽기 전용 DRM 문서, 압축 스트림, HWPX 등 다양한 문서 유형을 한번에 처리할 수 있는 도구가 필요했습니다.

hwarang은 이러한 필요에서 출발하여 Rust로 작성되었으며, 병렬 처리를 통해 수만 건의 문서도 빠르게 처리할 수 있습니다.

## 지원 포맷

- **HWP** (OLE 바이너리) - 한/글 5.x 이상
- **HWPX** (ZIP/XML) - 한/글 최신 XML 기반 포맷
- **HWPML** (순수 XML)

## 주요 기능

- 매직 바이트 기반 포맷 자동 감지
- 압축/비압축 스트림 모두 지원
- 배포문서 복호화 (AES/ECB)
- 표를 마크다운 테이블로 변환
- 머리글/꼬리글, 각주/미주, 글상자, 숨은설명 추출
- rayon 기반 병렬 배치 처리

## 벤치마크

| 항목 | 결과 |
|------|------|
| 파일 수 | 대부분 정부 공고문 형태의 49,353개 (HWP/HWPX) |
| 총 용량 | 1.0 GB |
| 소요 시간 | 47.49초 |
| 처리 속도 | 1,039 files/s |
| 성공률 | 99.94% (49,321/49,353) |
| 환경 | Apple M1, 16GB RAM, 8코어 |

실패 32건은 hwarang의 문제가 아닌 원본 파일 자체의 문제입니다:

| 실패 유형 | 설명 |
|-----------|------|
| 빈 파일 (0 bytes) | 다운로드 실패 등으로 파일 내용이 없음 |
| 확장자 불일치 | `.hwp` 확장자이지만 실제로는 HTML 또는 일반 텍스트 |
| DRM 래핑 | 소프트캠프(SCDSA), DOCUMENTSAFER 등 문서보안 솔루션이 씌워진 파일 |
| 깨진 XML | HWPML 형식이나 XML 구문 자체가 손상된 파일 |

## 설치

### CLI 도구로 설치

```bash
cargo install hwarang
```

### 라이브러리로 사용

`Cargo.toml`에 추가:

```toml
[dependencies]
hwarang = "0.1"
```

## 사용법

### CLI

```bash
# 단일 파일 텍스트 추출 (stdout 출력)
hwarang document.hwp

# 파일을 텍스트로 변환하여 저장
hwarang document.hwp -o output/

# 디렉토리 내 모든 HWP 파일 일괄 변환
hwarang ./documents/ -o ./output/

# 하위 디렉토리 포함 재귀 탐색
hwarang ./documents/ -o ./output/ -r

# 병렬 스레드 수 지정
hwarang ./documents/ -o ./output/ -r -j 8

# OLE 스트림 목록 확인
hwarang document.hwp --list-streams
```

### 라이브러리

```rust
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let text = hwarang::extract_text_from_file(Path::new("document.hwp"))?;
    println!("{}", text);
    Ok(())
}
```

## License

MIT License

Copyright (c) 2026 Lee Wonsup (이원섭) <onesup.lee@gmail.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
