//! `STATEA.BIN` / `STATEB.BIN`: runtime counters as a fixed 64-byte record, written to the
//! two files alternately. A record carries a sequence number and a CRC-32, so a write torn
//! by a power cut leaves the other file intact; at boot the newest valid record wins.
//!
//! Both files are preallocated to [`RECORD_LEN`] and overwritten in place, so a save only
//! touches the data sector, never the FAT or the cluster chain.

use crate::crc::crc32;

pub const RECORD_LEN: usize = 64;
const MAGIC: [u8; 4] = *b"HSTA";
const VERSION: u8 = 1;

/// Counters that must survive a reboot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeState {
    /// Fan running time since the last filter change.
    pub filter_elapsed_s: u32,
    /// Recovered heating / cooling energy in joules ([`openkwl_core::heat::EnergyCounter`]).
    pub heating_j: u64,
    pub cooling_j: u64,
    /// Time with at least one fan running.
    pub operating_s: u64,
    /// Base mode at the last save: 0 = manual, 1 = auto.
    pub auto_mode: bool,
    pub manual_level: u8,
}

/// Without a saved record (new card, no card, both records damaged) the unit follows the
/// weekly program; a manual choice starts from level 2. Never "manual, level 0": that would
/// keep the fans off until someone notices.
impl Default for RuntimeState {
    fn default() -> Self {
        Self { filter_elapsed_s: 0, heating_j: 0, cooling_j: 0, operating_s: 0, auto_mode: true, manual_level: 2 }
    }
}

/// Which file to write next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub const fn file_name(self) -> &'static str {
        match self {
            Slot::A => crate::STATE_A,
            Slot::B => crate::STATE_B,
        }
    }

    const fn for_seq(seq: u32) -> Self {
        if seq.is_multiple_of(2) { Slot::A } else { Slot::B }
    }
}

pub fn encode(state: &RuntimeState, seq: u32) -> [u8; RECORD_LEN] {
    let mut r = [0u8; RECORD_LEN];
    r[0..4].copy_from_slice(&MAGIC);
    r[4] = VERSION;
    r[5] = u8::from(state.auto_mode);
    r[6] = state.manual_level;
    r[8..12].copy_from_slice(&seq.to_le_bytes());
    r[12..16].copy_from_slice(&state.filter_elapsed_s.to_le_bytes());
    r[16..24].copy_from_slice(&state.heating_j.to_le_bytes());
    r[24..32].copy_from_slice(&state.cooling_j.to_le_bytes());
    r[32..40].copy_from_slice(&state.operating_s.to_le_bytes());
    let crc = crc32(&r[..RECORD_LEN - 4]);
    r[RECORD_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    r
}

/// `(state, seq)` of a valid record.
pub fn decode(bytes: &[u8]) -> Option<(RuntimeState, u32)> {
    if bytes.len() < RECORD_LEN || bytes[0..4] != MAGIC || bytes[4] != VERSION {
        return None;
    }
    let r = &bytes[..RECORD_LEN];
    let u32_at = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
    let u64_at = |o: usize| u64::from(u32_at(o)) | (u64::from(u32_at(o + 4)) << 32);
    if crc32(&r[..RECORD_LEN - 4]) != u32_at(RECORD_LEN - 4) {
        return None;
    }
    let state = RuntimeState {
        filter_elapsed_s: u32_at(12),
        heating_j: u64_at(16),
        cooling_j: u64_at(24),
        operating_s: u64_at(32),
        auto_mode: r[5] != 0,
        // A level the fans don't have (a foreign or damaged record) would give 0 V: clamp it.
        manual_level: r[6].min(openkwl_core::mode::MAX_LEVEL),
    };
    Some((state, u32_at(8)))
}

/// Result of reading both files at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loaded {
    /// Newest valid state, or the default when neither file is valid.
    pub state: RuntimeState,
    /// Sequence number for the next save.
    pub next_seq: u32,
}

impl Loaded {
    /// Picks the newest valid record of the two files (`None` = missing/unreadable).
    pub fn from_files(a: Option<&[u8]>, b: Option<&[u8]>) -> Self {
        let newest = [a.and_then(decode), b.and_then(decode)].into_iter().flatten().max_by_key(|&(_, seq)| seq);
        match newest {
            Some((state, seq)) => Self { state, next_seq: seq.wrapping_add(1) },
            None => Self { state: RuntimeState::default(), next_seq: 0 },
        }
    }

    /// Encodes the next record and the file it goes to; call [`Loaded::saved`] after a
    /// successful write.
    pub fn next_record(&self, state: &RuntimeState) -> (Slot, [u8; RECORD_LEN]) {
        (Slot::for_seq(self.next_seq), encode(state, self.next_seq))
    }

    pub fn saved(&mut self, state: RuntimeState) {
        self.state = state;
        self.next_seq = self.next_seq.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: RuntimeState = RuntimeState {
        filter_elapsed_s: 3_456_789,
        heating_j: 44_444_444_444,
        cooling_j: 12_345,
        operating_s: 374_025_600,
        auto_mode: true,
        manual_level: 2,
    };

    #[test]
    fn round_trip() {
        assert_eq!(decode(&encode(&S, 41)), Some((S, 41)));
    }

    #[test]
    fn rejects_corruption() {
        let mut r = encode(&S, 1);
        r[20] ^= 0x10;
        assert_eq!(decode(&r), None);
        assert_eq!(decode(&encode(&S, 1)[..63]), None);
        assert_eq!(decode(&[0xff; 64]), None, "erased");
    }

    #[test]
    fn manual_level_is_clamped() {
        let r = encode(&RuntimeState { manual_level: 9, ..S }, 3);
        assert_eq!(r[6], 9);
        assert_eq!(decode(&r), Some((RuntimeState { manual_level: 4, ..S }, 3)), "counters kept, level 0-4");
    }

    #[test]
    fn newest_valid_wins_and_slots_alternate() {
        let older = RuntimeState { filter_elapsed_s: 1, ..S };
        let a = encode(&older, 6);
        let b = encode(&S, 7);
        let l = Loaded::from_files(Some(&a), Some(&b));
        assert_eq!((l.state, l.next_seq), (S, 8));
        assert_eq!(l.next_record(&S).0, Slot::A, "seq 8 goes to A, keeping B (7) intact");

        let mut torn = b;
        torn[30] ^= 1;
        let l = Loaded::from_files(Some(&a), Some(&torn));
        assert_eq!((l.state, l.next_seq), (older, 7), "torn newer write falls back to the older file");
        assert_eq!(l.next_record(&S).0, Slot::B, "overwrites the torn file");

        let fresh = Loaded::from_files(None, None);
        assert_eq!((fresh.state, fresh.next_seq), (RuntimeState::default(), 0));
        assert!(fresh.state.auto_mode && fresh.state.manual_level == 2, "no record: Auto, never fans off");
    }

    #[test]
    fn saving_advances() {
        let mut l = Loaded::from_files(None, None);
        let (slot, rec) = l.next_record(&S);
        assert_eq!((slot, slot.file_name()), (Slot::A, "STATEA.BIN"));
        l.saved(S);
        let (slot2, rec2) = l.next_record(&S);
        assert_eq!(slot2, Slot::B);
        assert_eq!(Loaded::from_files(Some(&rec), Some(&rec2)).next_seq, 2);
    }
}
