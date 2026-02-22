use std::io::Read;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{HwpError, Result};

/// HWP 파일 시그니처 (32 bytes)
const HWP_SIGNATURE: &[u8; 32] = b"HWP Document File\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

/// FileHeader 플래그 비트
const FLAG_COMPRESSED: u32 = 1 << 0;
const FLAG_PASSWORD: u32 = 1 << 1;
const FLAG_DISTRIBUTION: u32 = 1 << 2;

#[derive(Debug, Clone)]
pub struct FileHeader {
    pub version: FileVersion,
    pub compressed: bool,
    pub password: bool,
    pub distribution: bool,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct FileVersion {
    pub major: u8,
    pub minor: u8,
    pub build: u8,
    pub revision: u8,
}

impl FileVersion {
    pub fn from_u32(v: u32) -> Self {
        FileVersion {
            major: ((v >> 24) & 0xFF) as u8,
            minor: ((v >> 16) & 0xFF) as u8,
            build: ((v >> 8) & 0xFF) as u8,
            revision: (v & 0xFF) as u8,
        }
    }
}

impl std::fmt::Display for FileVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.build, self.revision
        )
    }
}

impl FileHeader {
    /// FileHeader 스트림에서 파싱
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        // 시그니처 32바이트 검증
        let mut sig = [0u8; 32];
        reader.read_exact(&mut sig)?;
        if &sig != HWP_SIGNATURE {
            return Err(HwpError::InvalidSignature);
        }

        // 버전 4바이트 (u32 LE)
        let version_raw = reader.read_u32::<LittleEndian>()?;
        let version = FileVersion::from_u32(version_raw);

        // 플래그 4바이트 (u32 LE)
        let flags = reader.read_u32::<LittleEndian>()?;
        let compressed = (flags & FLAG_COMPRESSED) != 0;
        let password = (flags & FLAG_PASSWORD) != 0;
        let distribution = (flags & FLAG_DISTRIBUTION) != 0;

        if password {
            return Err(HwpError::PasswordProtected);
        }

        Ok(FileHeader {
            version,
            compressed,
            password,
            distribution,
            flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_version_from_u32() {
        let v = FileVersion::from_u32(0x05010207);
        assert_eq!(v.major, 5);
        assert_eq!(v.minor, 1);
        assert_eq!(v.build, 2);
        assert_eq!(v.revision, 7);
    }

    #[test]
    fn test_invalid_signature() {
        let data = vec![0u8; 40];
        let result = FileHeader::from_reader(&mut &data[..]);
        assert!(matches!(result, Err(HwpError::InvalidSignature)));
    }
}
