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

    fn sample_path(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("hwplib")
            .join("sample_hwp")
            .join(name)
    }

    #[test]
    fn test_list_streams_blank_hwp() {
        let path = sample_path("basic/blank.hwp");
        if !path.exists() {
            eprintln!("Skipping test: {:?} not found", path);
            return;
        }
        let streams = list_streams(&path).unwrap();
        assert!(!streams.is_empty());

        let names: Vec<&str> = streams.iter().map(|s| s.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("FileHeader")),
            "FileHeader not found in: {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n.contains("DocInfo")),
            "DocInfo not found in: {:?}",
            names
        );
    }

    #[test]
    fn test_extract_text_blank_hwp() {
        let path = sample_path("basic/blank.hwp");
        if !path.exists() {
            eprintln!("Skipping test: {:?} not found", path);
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(text.contains("HWP version: 5."));
        assert!(text.contains("Compressed: false"));
    }

    #[test]
    fn test_fileheader_compressed_hwp() {
        // blank.hwp (root) has Scripts etc, check version parsing
        let path = sample_path("blank.hwp");
        if !path.exists() {
            eprintln!("Skipping test: {:?} not found", path);
            return;
        }
        let text = extract_text_from_file(&path).unwrap();
        assert!(text.contains("HWP version: 5."));
        // Root blank.hwp also has BodyText/Section0
        assert!(text.contains("BodyText"));
    }
}
