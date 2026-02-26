pub mod error;
pub mod extract;
pub mod hwp;
pub mod hwpx;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::error::{HwpError, Result};
use crate::extract as text_extract;
use crate::hwp::crypto;
use crate::hwp::docinfo;
use crate::hwp::header::FileHeader;
use crate::hwp::record;
use crate::hwp::stream;

/// Extracts text content from an HWP or HWPX document file.
///
/// Automatically detects the file format by reading the magic bytes:
/// - `D0 CF 11 E0` — HWP (OLE compound document)
/// - `50 4B 03 04` — HWPX (ZIP-based OWPML)
/// - `3C 3F 78 6D` — HWPML (plain XML)
///
/// # Errors
///
/// Returns [`HwpError::UnsupportedFormat`] if the file is too short or has
/// unrecognised magic bytes. Other variants may be returned for I/O failures,
/// invalid structures, or password-protected documents.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// let text = hwarang::extract_text_from_file(Path::new("document.hwp"))?;
/// println!("{text}");
/// # Ok::<(), hwarang::error::HwpError>(())
/// ```
pub fn extract_text_from_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    drop(file);

    if n < 4 {
        return Err(HwpError::UnsupportedFormat);
    }

    match magic {
        [0x50, 0x4B, 0x03, 0x04] => hwpx::extract_text_from_hwpx(path), // ZIP (HWPX)
        [0xD0, 0xCF, 0x11, 0xE0] => extract_text_from_hwp(path),        // OLE (HWP)
        [0x3C, 0x3F, 0x78, 0x6D] => hwpx::extract_text_from_hwpml(path), // <?xml (HWPML)
        _ => Err(HwpError::UnsupportedFormat),
    }
}

/// HWP(OLE 컨테이너) 파일에서 텍스트를 추출한다.
///
/// 섹션별 병렬 처리: CFB 스트림 I/O 후 압축해제·파싱·텍스트 추출을
/// rayon으로 병렬 수행한다.
fn extract_text_from_hwp(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut comp = cfb::CompoundFile::open(file)?;

    // FileHeader 스트림 읽기
    let header = {
        let mut stream = comp
            .open_stream("/FileHeader")
            .map_err(|_| HwpError::StreamNotFound("FileHeader".into()))?;
        FileHeader::from_reader(&mut stream)?
    };

    // DocInfo에서 section_count 파싱
    let doc_info = {
        let mut s = comp
            .open_stream("/DocInfo")
            .map_err(|_| HwpError::StreamNotFound("DocInfo".into()))?;
        let data = stream::read_and_decompress(&mut s, header.compressed)?;
        let records = record::read_records(&data)?;
        docinfo::parse_doc_info(&records)?
    };

    let storage = if header.distribution {
        "ViewText"
    } else {
        "BodyText"
    };

    // Phase 1: 모든 섹션의 raw 스트림 데이터를 순차 읽기 (CFB I/O)
    let mut section_raw: Vec<(u16, Vec<u8>)> = Vec::new();
    for i in 0..doc_info.section_count {
        let stream_name = format!("/{}/Section{}", storage, i);
        let mut s = match comp.open_stream(&stream_name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let raw = stream::read_stream_data(&mut s)?;
        section_raw.push((i, raw));
    }

    // Phase 2: 섹션별 병렬 처리 (압축해제 + 레코드 파싱 + 텍스트 추출)
    let compressed = header.compressed;
    let distribution = header.distribution;

    let mut section_texts: Vec<(u16, String)> = section_raw
        .into_par_iter()
        .map(|(i, raw)| {
            let data = if distribution {
                let decrypted = crypto::decrypt_distribution_stream(&raw)?;
                if compressed {
                    stream::decompress(&decrypted)?
                } else {
                    decrypted
                }
            } else if compressed {
                stream::decompress(&raw)?
            } else {
                raw
            };

            let records = record::read_records(&data)?;
            let mut text = String::new();
            text_extract::extract_section_text(&records, &mut text);
            Ok((i, text))
        })
        .collect::<Result<Vec<_>>>()?;

    // Phase 3: 섹션 순서대로 병합
    section_texts.sort_unstable_by_key(|(i, _)| *i);
    let text = section_texts
        .into_iter()
        .map(|(_, t)| t)
        .collect::<String>();

    Ok(text)
}

/// Lists all streams inside an OLE compound file.
///
/// Useful for inspecting the internal structure of an HWP file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or is not a valid OLE
/// compound document.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// let streams = hwarang::list_streams(Path::new("document.hwp"))?;
/// for s in &streams {
///     println!("{s}");
/// }
/// # Ok::<(), hwarang::error::HwpError>(())
/// ```
pub fn list_streams(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let comp = cfb::CompoundFile::open(file)?;
    Ok(comp
        .walk()
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect())
}

/// The outcome of extracting text from a single file in a batch operation.
///
/// Used by [`extract_text_batch`] to report per-file success or failure
/// without aborting the entire batch.
#[derive(Debug)]
pub struct BatchResult {
    /// The path of the processed file.
    pub path: PathBuf,
    /// Extracted text on success, or the error that occurred.
    pub result: Result<String>,
}

/// Extracts text from multiple HWP/HWPX files in parallel.
///
/// Every file is processed concurrently using rayon's work-stealing
/// scheduler. Within each file, sections are also processed in parallel
/// (see [`extract_text_from_file`]), so all available CPU cores are
/// fully utilised even when the input contains only a handful of
/// large documents.
///
/// The returned [`Vec<BatchResult>`] preserves the input order.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
///
/// let paths = vec![
///     PathBuf::from("a.hwp"),
///     PathBuf::from("b.hwpx"),
/// ];
/// let results = hwarang::extract_text_batch(&paths);
/// for br in &results {
///     match &br.result {
///         Ok(text) => println!("{}: {} chars", br.path.display(), text.len()),
///         Err(e) => eprintln!("{}: {}", br.path.display(), e),
///     }
/// }
/// ```
pub fn extract_text_batch(paths: &[PathBuf]) -> Vec<BatchResult> {
    paths
        .par_iter()
        .map(|path| BatchResult {
            path: path.clone(),
            result: extract_text_from_file(path),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwp::{docinfo, record, stream};

    fn sample_path(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("hwplib")
            .join("sample_hwp")
            .join(name)
    }

    fn open_hwp(name: &str) -> Option<(cfb::CompoundFile<File>, FileHeader)> {
        let path = sample_path(name);
        if !path.exists() {
            eprintln!("Skipping: {:?} not found", path);
            return None;
        }
        let file = File::open(&path).unwrap();
        let mut comp = cfb::CompoundFile::open(file).unwrap();
        let header = {
            let mut s = comp.open_stream("/FileHeader").unwrap();
            FileHeader::from_reader(&mut s).unwrap()
        };
        Some((comp, header))
    }

    #[test]
    fn test_list_streams_blank_hwp() {
        let path = sample_path("basic/blank.hwp");
        if !path.exists() {
            return;
        }
        let streams = list_streams(&path).unwrap();
        assert!(!streams.is_empty());
        let names: Vec<&str> = streams.iter().map(|s| s.as_str()).collect();
        assert!(names.iter().any(|n| n.contains("FileHeader")));
        assert!(names.iter().any(|n| n.contains("DocInfo")));
    }

    #[test]
    fn test_extract_text_blank_hwp() {
        let path = sample_path("basic/blank.hwp");
        if !path.exists() {
            return;
        }
        // blank.hwp는 빈 문서이므로 빈 텍스트 또는 줄바꿈만
        let text = extract_text_from_file(&path).unwrap();
        assert!(text.trim().is_empty() || text.chars().all(|c| c.is_whitespace()));
    }

    #[test]
    fn test_extract_text_table_hwp() {
        let path = sample_path("basic/표.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        // 표 안의 셀 텍스트도 추출되어야 함
        assert!(!text.trim().is_empty(), "Table HWP should have text");
        eprintln!("=== 표.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_header_footer_hwp() {
        let path = sample_path("basic/머리글꼬리글.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(!text.trim().is_empty());
        eprintln!("=== 머리글꼬리글.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_footnote_hwp() {
        let path = sample_path("basic/각주미주.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(!text.trim().is_empty());
        eprintln!("=== 각주미주.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_hidden_comment_hwp() {
        let path = sample_path("basic/숨은설명.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(!text.trim().is_empty());
        eprintln!("=== 숨은설명.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_textbox_hwp() {
        let path = sample_path("basic/글상자.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(!text.trim().is_empty());
        eprintln!("=== 글상자.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_equation_hwp() {
        let path = sample_path("basic/수식.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        eprintln!("=== 수식.hwp ===\n{}", text);
    }

    #[test]
    fn test_extract_text_distribution_hwp() {
        let path = sample_path("distribution.hwp");
        if !path.exists() {
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        eprintln!("=== distribution.hwp ===\n{}", text);
        // 배포문서도 텍스트 추출이 가능해야 함
    }

    #[test]
    fn test_extract_text_compressed() {
        // superboard 실제 compressed 파일
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("superboard/data/raw/files/20160322124");
        if !path.exists() {
            return;
        }
        let hwp_path = std::fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().map_or(false, |ext| ext == "hwp"))
            .map(|e| e.path());

        let Some(hwp_path) = hwp_path else { return };
        let text = extract_text_from_file(&hwp_path).unwrap();
        // 실제 문서이므로 텍스트가 있어야 함
        assert!(!text.trim().is_empty(), "Compressed HWP should have text");
        let preview: String = text.chars().take(500).collect();
        eprintln!("Compressed HWP text (first 500 chars):\n{}", preview);
    }

    #[test]
    fn test_docinfo_section_count() {
        let Some((mut comp, header)) = open_hwp("basic/blank.hwp") else {
            return;
        };
        let mut s = comp.open_stream("/DocInfo").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();
        let info = docinfo::parse_doc_info(&records).unwrap();
        assert_eq!(info.section_count, 1);
    }

    #[test]
    fn test_docinfo_records() {
        let Some((mut comp, header)) = open_hwp("basic/blank.hwp") else {
            return;
        };
        let mut s = comp.open_stream("/DocInfo").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();

        assert!(!records.is_empty());
        // 첫 번째 레코드는 DOCUMENT_PROPERTIES
        assert_eq!(records[0].header.tag_id, record::HWPTAG_DOCUMENT_PROPERTIES);
        assert_eq!(records[0].header.level, 0);
    }

    #[test]
    fn test_section0_records() {
        let Some((mut comp, header)) = open_hwp("basic/blank.hwp") else {
            return;
        };
        let mut s = comp.open_stream("/BodyText/Section0").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();

        assert!(!records.is_empty());
        // Section은 PARA_HEADER로 시작
        assert_eq!(records[0].header.tag_id, record::HWPTAG_PARA_HEADER);
    }

    #[test]
    fn test_compressed_file_records() {
        // superboard의 실제 compressed 파일 테스트
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("superboard/data/raw/files/20160322124");
        if !path.exists() {
            return;
        }
        // 디렉토리 안의 첫 번째 .hwp 파일 찾기
        let hwp_path = std::fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().map_or(false, |ext| ext == "hwp"))
            .map(|e| e.path());

        let Some(hwp_path) = hwp_path else { return };

        let file = File::open(&hwp_path).unwrap();
        let mut comp = cfb::CompoundFile::open(file).unwrap();
        let header = {
            let mut s = comp.open_stream("/FileHeader").unwrap();
            FileHeader::from_reader(&mut s).unwrap()
        };
        assert!(header.compressed);

        // DocInfo 레코드 파싱
        let mut s = comp.open_stream("/DocInfo").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();
        assert!(!records.is_empty());
        assert_eq!(records[0].header.tag_id, record::HWPTAG_DOCUMENT_PROPERTIES);

        // Section0 레코드 파싱
        let mut s = comp.open_stream("/BodyText/Section0").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();
        assert!(!records.is_empty());
        assert_eq!(records[0].header.tag_id, record::HWPTAG_PARA_HEADER);
    }

    #[test]
    fn test_unsupported_format() {
        // 임시 파일에 잘못된 매직 바이트 기록
        let dir = std::env::temp_dir();
        let path = dir.join("test_unsupported.bin");
        std::fs::write(&path, b"NOT_A_VALID_FORMAT").unwrap();
        let result = extract_text_from_file(&path);
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_nonexistent_file() {
        let path = Path::new("/tmp/does_not_exist_hwp_test_12345.hwp");
        let result = extract_text_from_file(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_too_short() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_too_short.hwp");
        std::fs::write(&path, b"AB").unwrap(); // 4바이트 미만
        let result = extract_text_from_file(&path);
        assert!(matches!(result, Err(HwpError::UnsupportedFormat)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_empty_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_empty.hwp");
        std::fs::write(&path, b"").unwrap();
        let result = extract_text_from_file(&path);
        assert!(matches!(result, Err(HwpError::UnsupportedFormat)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_list_streams_nonexistent() {
        let path = Path::new("/tmp/does_not_exist_hwp_test_12345.hwp");
        let result = list_streams(path);
        assert!(result.is_err());
    }
}
