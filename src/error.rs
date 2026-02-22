use thiserror::Error;

#[derive(Error, Debug)]
pub enum HwpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid HWP signature")]
    InvalidSignature,

    #[error("Unsupported HWP version: {0}.{1}.{2}.{3}")]
    UnsupportedVersion(u8, u8, u8, u8),

    #[error("Password-protected document")]
    PasswordProtected,

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("Invalid record header")]
    InvalidRecordHeader,

    #[error("Decompression failed: {0}")]
    DecompressFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptFailed(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, HwpError>;
