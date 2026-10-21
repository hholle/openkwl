//! `AUTH.BIN`: magic, version, [`PasswordRecord`] and CRC-32.

use openkwl_auth::{PasswordRecord, RECORD_LEN};

use crate::crc::crc32;

const MAGIC: [u8; 4] = *b"HAUT";
const VERSION: u8 = 1;
pub const LEN: usize = 4 + 1 + RECORD_LEN + 4;

pub fn encode(record: &PasswordRecord) -> [u8; LEN] {
    let mut out = [0u8; LEN];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5..5 + RECORD_LEN].copy_from_slice(&record.to_bytes());
    let crc = crc32(&out[..LEN - 4]);
    out[LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    out
}

/// A password nobody knows: what the platforms use when `AUTH.BIN` and `AUTH.BAK` exist but
/// can't be used (empty, damaged, card error), or there is no card. Login fails and first setup
/// stays closed, instead of opening setup to anyone on the network; an empty `RESETPW` file on
/// the card opens it. (Its hash is all zeros, which no PBKDF2 output equals in practice.)
pub const UNKNOWN: PasswordRecord =
    PasswordRecord { salt: [0; openkwl_auth::SALT_LEN], iterations: 1, hash: [0; openkwl_auth::HASH_LEN] };

/// `None` for a truncated or corrupt file (see [`UNKNOWN`]).
pub fn decode(bytes: &[u8]) -> Option<PasswordRecord> {
    if bytes.len() != LEN || bytes[..4] != MAGIC || bytes[4] != VERSION {
        return None;
    }
    let crc = u32::from_le_bytes(bytes[LEN - 4..].try_into().ok()?);
    if crc32(&bytes[..LEN - 4]) != crc {
        return None;
    }
    Some(PasswordRecord::from_bytes(bytes[5..5 + RECORD_LEN].try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_corruption() {
        let rec = PasswordRecord { salt: [7; 16], iterations: 10_000, hash: [9; 32] };
        let bytes = encode(&rec);
        assert_eq!(decode(&bytes), Some(rec));
        let mut bad = bytes;
        bad[20] ^= 1;
        assert_eq!(decode(&bad), None);
        assert_eq!(decode(&bytes[..LEN - 1]), None);
        assert_eq!(decode(&[]), None);
    }
}
