use thiserror::Error;

/// Errors that can occur while reading or parsing HWP/HWPX documents.
#[derive(Error, Debug)]
pub enum HwpError {
    /// An I/O error occurred while reading the file or stream.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The file does not start with the expected HWP OLE signature.
    #[error("Invalid HWP signature")]
    InvalidSignature,

    /// The document uses an HWP version that this library does not support.
    #[error("Unsupported HWP version: {0}.{1}.{2}.{3}")]
    UnsupportedVersion(u8, u8, u8, u8),

    /// The document is password-protected and cannot be read.
    #[error("Password-protected document")]
    PasswordProtected,

    /// A required OLE stream was not found in the compound file.
    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    /// A record header could not be parsed (truncated or malformed data).
    #[error("Invalid record header")]
    InvalidRecordHeader,

    /// Zlib decompression of a stream or record body failed.
    #[error("Decompression failed: {0}")]
    DecompressFailed(String),

    /// AES decryption of a distribution-document stream failed.
    #[error("Decryption failed: {0}")]
    DecryptFailed(String),

    /// A general parse error for malformed or unexpected data.
    #[error("Parse error: {0}")]
    Parse(String),

    /// The file is not a recognised HWP, HWPX, or HWPML format.
    #[error("Unsupported file format")]
    UnsupportedFormat,

    /// An error specific to HWPX (ZIP/XML) processing.
    #[error("HWPX error: {0}")]
    Hwpx(String),
}

/// A specialised `Result` type for HWP operations.
pub type Result<T> = std::result::Result<T, HwpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_io_error() {
        let err = HwpError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let msg = err.to_string();
        assert!(msg.contains("I/O error"), "got: {msg}");
    }

    #[test]
    fn test_display_invalid_signature() {
        let msg = HwpError::InvalidSignature.to_string();
        assert_eq!(msg, "Invalid HWP signature");
    }

    #[test]
    fn test_display_unsupported_version() {
        let msg = HwpError::UnsupportedVersion(5, 1, 2, 7).to_string();
        assert_eq!(msg, "Unsupported HWP version: 5.1.2.7");
    }

    #[test]
    fn test_display_password_protected() {
        let msg = HwpError::PasswordProtected.to_string();
        assert_eq!(msg, "Password-protected document");
    }

    #[test]
    fn test_display_stream_not_found() {
        let msg = HwpError::StreamNotFound("DocInfo".into()).to_string();
        assert_eq!(msg, "Stream not found: DocInfo");
    }

    #[test]
    fn test_display_unsupported_format() {
        let msg = HwpError::UnsupportedFormat.to_string();
        assert_eq!(msg, "Unsupported file format");
    }

    #[test]
    fn test_display_hwpx() {
        let msg = HwpError::Hwpx("bad zip".into()).to_string();
        assert_eq!(msg, "HWPX error: bad zip");
    }

    #[test]
    fn test_display_decrypt_failed() {
        let msg = HwpError::DecryptFailed("bad key".into()).to_string();
        assert_eq!(msg, "Decryption failed: bad key");
    }

    #[test]
    fn test_display_decompress_failed() {
        let msg = HwpError::DecompressFailed("corrupt".into()).to_string();
        assert_eq!(msg, "Decompression failed: corrupt");
    }

    #[test]
    fn test_display_parse() {
        let msg = HwpError::Parse("unexpected".into()).to_string();
        assert_eq!(msg, "Parse error: unexpected");
    }

    #[test]
    fn test_display_invalid_record_header() {
        let msg = HwpError::InvalidRecordHeader.to_string();
        assert_eq!(msg, "Invalid record header");
    }
}
