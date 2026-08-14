use crate::error::{HwpError, Result};

/// 리틀엔디언 바이트 커서.
///
/// HWP 3.0은 스트림 전체를 순차 소비하는 포맷이라 offset 기반 커서로 파싱한다.
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn eof_err() -> HwpError {
        HwpError::Parse("unexpected end of HWP 3.0 stream".into())
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or_else(Self::eof_err)?;
        self.pos += 1;
        Ok(b)
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let end = self.pos.checked_add(2).ok_or_else(Self::eof_err)?;
        let bytes = self.data.get(self.pos..end).ok_or_else(Self::eof_err)?;
        self.pos = end;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let end = self.pos.checked_add(4).ok_or_else(Self::eof_err)?;
        let bytes = self.data.get(self.pos..end).ok_or_else(Self::eof_err)?;
        self.pos = end;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(Self::eof_err)?;
        let bytes = self.data.get(self.pos..end).ok_or_else(Self::eof_err)?;
        self.pos = end;
        Ok(bytes)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        let end = self.pos.checked_add(n).ok_or_else(Self::eof_err)?;
        if end > self.data.len() {
            return Err(Self::eof_err());
        }
        self.pos = end;
        Ok(())
    }

    /// 남은 바이트 전체.
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }
}
