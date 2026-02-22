use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyInit};
use aes::Aes128;

use crate::error::{HwpError, Result};

type Aes128EcbDec = ecb::Decryptor<Aes128>;

/// LCG (Linear Congruential Generator) XOR 디옵퓨스케이션
///
/// 배포문서의 256바이트 암호화 헤더를 해독한다.
/// Java 원본: Obfuscation.transform()
///
/// 핵심: i=0부터 반복하되 XOR는 i>=4에서만 적용.
/// 0~3번 바이트는 seed로 사용되지만 number 카운터도 소비한다.
fn deobfuscate(data: &mut [u8; 256]) {
    // Java의 LittleEndian.getInt() → signed int32
    let mut random_seed: i32 =
        i32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    let mut value: u8 = 0;
    let mut number: i32 = 0;

    for i in 0..256 {
        if number == 0 {
            // value() = (byte)(rand() & 0xFF)
            random_seed = random_seed.wrapping_mul(214013).wrapping_add(2531011);
            value = ((random_seed >> 16) & 0x7FFF & 0xFF) as u8;
            // number() = (rand() & 0xF) + 1
            random_seed = random_seed.wrapping_mul(214013).wrapping_add(2531011);
            number = ((random_seed >> 16) & 0x7FFF & 0xF) + 1;
        }

        if i >= 4 {
            data[i] ^= value;
        }

        number -= 1;
    }
}

/// 배포문서 스트림을 복호화한다.
///
/// 스트림 구조:
/// 1. 4바이트 레코드 헤더 (스킵)
/// 2. 256바이트 암호화 메타데이터 (LCG XOR 디옵퓨스케이션)
/// 3. 나머지: AES/ECB/NoPadding 암호화된 데이터
pub fn decrypt_distribution_stream(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 260 {
        return Err(HwpError::DecryptFailed(
            "Distribution stream too short".into(),
        ));
    }

    // 4바이트 레코드 헤더 스킵 + 256바이트 메타데이터
    let mut meta = [0u8; 256];
    meta.copy_from_slice(&data[4..260]);

    // LCG XOR 디옵퓨스케이션
    deobfuscate(&mut meta);

    // AES 키 추출: offset = 4 + (meta[0] & 0xF), 16바이트
    let key_offset = 4 + (meta[0] & 0xF) as usize;
    if key_offset + 16 > 256 {
        return Err(HwpError::DecryptFailed("Key offset out of range".into()));
    }
    let key = &meta[key_offset..key_offset + 16];

    // 나머지 데이터를 AES/ECB/PKCS7로 복호화
    let encrypted = &data[260..];
    if encrypted.is_empty() {
        return Ok(Vec::new());
    }

    let mut buf = encrypted.to_vec();

    let decryptor = Aes128EcbDec::new_from_slice(key)
        .map_err(|e| HwpError::DecryptFailed(format!("AES key init failed: {}", e)))?;

    let decrypted = decryptor
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| HwpError::DecryptFailed(format!("AES decrypt failed: {}", e)))?;

    Ok(decrypted.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deobfuscate_basic() {
        let mut data = [0u8; 256];
        deobfuscate(&mut data);
        // 최소한 패닉 없이 동작해야 함
    }

    #[test]
    fn test_decrypt_too_short() {
        let data = vec![0u8; 100];
        assert!(matches!(
            decrypt_distribution_stream(&data),
            Err(HwpError::DecryptFailed(_))
        ));
    }

}
