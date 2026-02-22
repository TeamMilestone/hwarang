pub mod error;
pub mod extract;
pub mod hwp;

use std::fs::File;
use std::path::Path;

use crate::error::{HwpError, Result};
use crate::hwp::header::FileHeader;

/// HWP 파일에서 텍스트를 추출한다.
pub fn extract_text_from_file(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut comp = cfb::CompoundFile::open(file)?;

    // FileHeader 스트림 읽기
    let header = {
        let mut stream = comp
            .open_stream("/FileHeader")
            .map_err(|_| HwpError::StreamNotFound("FileHeader".into()))?;
        FileHeader::from_reader(&mut stream)?
    };

    // 스트림 목록 출력 (디버그용, 이후 이터레이션에서 실제 추출로 대체)
    let mut text = String::new();
    text.push_str(&format!("HWP version: {}\n", header.version));
    text.push_str(&format!("Compressed: {}\n", header.compressed));
    text.push_str(&format!("Distribution: {}\n", header.distribution));

    // OLE 스트림 목록
    let entries: Vec<String> = comp
        .walk()
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();

    text.push_str(&format!("Streams: {}\n", entries.len()));
    for entry in &entries {
        text.push_str(&format!("  {}\n", entry));
    }

    Ok(text)
}

/// OLE 컨테이너의 스트림 목록을 반환한다.
pub fn list_streams(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let comp = cfb::CompoundFile::open(file)?;
    Ok(comp
        .walk()
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwp::record;
    use crate::hwp::stream;

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
        let text = extract_text_from_file(&path).unwrap();
        assert!(text.contains("HWP version: 5."));
        assert!(text.contains("Compressed: false"));
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
        assert_eq!(
            records[0].header.tag_id,
            record::HWPTAG_DOCUMENT_PROPERTIES
        );
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
            .find(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "hwp")
            })
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
        assert_eq!(
            records[0].header.tag_id,
            record::HWPTAG_DOCUMENT_PROPERTIES
        );

        // Section0 레코드 파싱
        let mut s = comp.open_stream("/BodyText/Section0").unwrap();
        let data = stream::read_and_decompress(&mut s, header.compressed).unwrap();
        let records = record::read_records(&data).unwrap();
        assert!(!records.is_empty());
        assert_eq!(records[0].header.tag_id, record::HWPTAG_PARA_HEADER);
    }
}
