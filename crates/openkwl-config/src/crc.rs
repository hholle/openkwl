//! CRC-32 (IEEE 802.3, reflected, as in zlib) for the binary files.

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_value() {
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(super::crc32(b""), 0);
    }
}
