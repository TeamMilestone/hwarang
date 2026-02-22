use std::io::Read;

use flate2::read::DeflateDecoder;

use crate::error::{HwpError, Result};

/// 압축된 스트림 데이터를 raw deflate로 압축해제한다.
/// HWP는 zlib 헤더 없는 raw deflate를 사용한다.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| HwpError::DecompressFailed(e.to_string()))?;
    Ok(decompressed)
}

/// OLE 스트림에서 전체 데이터를 읽는다.
pub fn read_stream_data<R: Read>(stream: &mut R) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    stream.read_to_end(&mut data)?;
    Ok(data)
}

/// 압축 여부에 따라 스트림 데이터를 읽고 필요시 압축해제한다.
pub fn read_and_decompress<R: Read>(stream: &mut R, compressed: bool) -> Result<Vec<u8>> {
    let raw = read_stream_data(stream)?;
    if compressed {
        decompress(&raw)
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn test_decompress_roundtrip() {
        let original = b"Hello, HWP world! This is a test of raw deflate compression.";

        // 압축
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        // 압축해제
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_read_and_decompress_uncompressed() {
        let data = b"uncompressed data";
        let result = read_and_decompress(&mut &data[..], false).unwrap();
        assert_eq!(&result, data);
    }
}
