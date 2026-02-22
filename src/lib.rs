pub mod error;
pub mod extract;
pub mod hwp;

use std::fs::File;
use std::path::Path;

use crate::error::{HwpError, Result};
use crate::hwp::docinfo;
use crate::hwp::header::FileHeader;
use crate::hwp::para_text;
use crate::hwp::record::{self, Record};
use crate::hwp::stream;

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

    // DocInfo에서 section_count 파싱
    let doc_info = {
        let mut s = comp
            .open_stream("/DocInfo")
            .map_err(|_| HwpError::StreamNotFound("DocInfo".into()))?;
        let data = stream::read_and_decompress(&mut s, header.compressed)?;
        let records = record::read_records(&data)?;
        docinfo::parse_doc_info(&records)?
    };

    // 각 섹션에서 텍스트 추출
    let mut text = String::new();
    for i in 0..doc_info.section_count {
        let stream_name = format!("/BodyText/Section{}", i);
        let mut s = match comp.open_stream(&stream_name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let data = stream::read_and_decompress(&mut s, header.compressed)?;
        let records = record::read_records(&data)?;
        extract_section_text(&records, &mut text);
    }

    Ok(text)
}

/// 섹션 레코드 시퀀스에서 PARA_TEXT 레코드의 텍스트를 추출한다.
fn extract_section_text(records: &[Record], text: &mut String) {
    for rec in records {
        if rec.header.tag_id == record::HWPTAG_PARA_TEXT {
            let (para_text, _controls) = para_text::extract_text(&rec.data);
            text.push_str(&para_text);
            text.push('\n');
        }
    }
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
        // 표 안의 텍스트는 Iter hwp06에서 추출되므로, 본문 텍스트만 확인
        // 표.hwp에는 "표 예제" 등의 본문 텍스트가 있을 수 있음
        eprintln!("Table text:\n{}", text);
        // 최소한 에러 없이 추출되어야 함
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
        eprintln!("Compressed HWP text (first 500 chars):\n{}", &text[..text.len().min(500)]);
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
