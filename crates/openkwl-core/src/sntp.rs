//! SNTP client logic (RFC 4330): request packet, reply validation and retry policy.
//!
//! The firmware sends [`request`] over UDP port 123, passes the reply to [`parse`] and sets
//! the RTC. Precision of a second is plenty for schedules; half the measured round trip
//! can be added by the caller.

use crate::clock::MIN_VALID_UNIX_S;

pub const PACKET_LEN: usize = 48;
pub const PORT: u16 = 123;
/// Seconds from 1900-01-01 (NTP era 0) to 1970-01-01.
const NTP_TO_UNIX: u64 = 2_208_988_800;

/// Client request (version 4, mode 3). The transmit timestamp carries `nonce`, which a
/// genuine reply echoes as its origin timestamp; use fresh random bytes per request.
pub fn request(nonce: [u8; 8]) -> [u8; PACKET_LEN] {
    let mut p = [0u8; PACKET_LEN];
    p[0] = (4 << 3) | 3;
    p[40..48].copy_from_slice(&nonce);
    p
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    TooShort,
    /// Not a server/broadcast reply or unsupported version.
    NotServer,
    /// Stratum 0: kiss-of-death (e.g. `RATE`); back off.
    KissOfDeath,
    /// Server not synchronised (leap indicator 3 or stratum > 15).
    Unsynchronized,
    /// Origin timestamp does not echo our request (stale or spoofed).
    OriginMismatch,
    /// Time before 2024 or after 2099.
    Implausible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    /// Server transmit time, milliseconds since 1970-01-01 UTC.
    pub unix_ms: u64,
    pub stratum: u8,
}

pub fn parse(reply: &[u8], sent: &[u8; PACKET_LEN]) -> Result<Reply, Error> {
    if reply.len() < PACKET_LEN {
        return Err(Error::TooShort);
    }
    let leap = reply[0] >> 6;
    let version = (reply[0] >> 3) & 7;
    let mode = reply[0] & 7;
    if !(3..=4).contains(&version) || !(mode == 4 || mode == 5) {
        return Err(Error::NotServer);
    }
    let stratum = reply[1];
    if stratum == 0 {
        return Err(Error::KissOfDeath);
    }
    if leap == 3 || stratum > 15 {
        return Err(Error::Unsynchronized);
    }
    if reply[24..32] != sent[40..48] {
        return Err(Error::OriginMismatch);
    }
    let secs = u64::from(u32::from_be_bytes([reply[40], reply[41], reply[42], reply[43]]));
    let frac = u64::from(u32::from_be_bytes([reply[44], reply[45], reply[46], reply[47]]));
    // Era 1 starts 2036-02-07; small values belong to it.
    let ntp_secs = if secs < 0x8000_0000 { secs + (1 << 32) } else { secs };
    let unix_s = ntp_secs.checked_sub(NTP_TO_UNIX).ok_or(Error::Implausible)?;
    if !(MIN_VALID_UNIX_S..4_102_444_800).contains(&unix_s) {
        return Err(Error::Implausible);
    }
    Ok(Reply { unix_ms: unix_s * 1000 + ((frac * 1000) >> 32), stratum })
}

/// When to query next: hourly after success, exponential backoff 15 s … 15 min after
/// failures (and at least 15 min after a kiss-of-death).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncPolicy {
    next_s: u64,
    failures: u8,
    last_success_s: Option<u64>,
}

impl SyncPolicy {
    pub const RESYNC_S: u64 = 3600;
    pub const MIN_RETRY_S: u64 = 15;
    pub const MAX_RETRY_S: u64 = 900;

    /// Query right away after boot.
    pub const fn new() -> Self {
        Self { next_s: 0, failures: 0, last_success_s: None }
    }

    /// `now_s` is a monotonic clock (uptime).
    pub const fn due(&self, now_s: u64) -> bool {
        now_s >= self.next_s
    }

    pub fn on_success(&mut self, now_s: u64) {
        self.failures = 0;
        self.last_success_s = Some(now_s);
        self.next_s = now_s + Self::RESYNC_S;
    }

    pub fn on_failure(&mut self, now_s: u64, error: Option<Error>) {
        self.failures = self.failures.saturating_add(1);
        let backoff = (Self::MIN_RETRY_S << (self.failures - 1).min(6)).min(Self::MAX_RETRY_S);
        let backoff = if error == Some(Error::KissOfDeath) { Self::MAX_RETRY_S } else { backoff };
        self.next_s = now_s + backoff;
    }

    /// Uptime of the last successful sync.
    pub const fn last_success_s(&self) -> Option<u64> {
        self.last_success_s
    }
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONCE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    /// Server reply for 2026-10-16T21:41:02.5Z echoing `NONCE`.
    fn reply() -> [u8; PACKET_LEN] {
        let mut r = [0u8; PACKET_LEN];
        r[0] = (4 << 3) | 4;
        r[1] = 2;
        r[24..32].copy_from_slice(&NONCE);
        let ntp = (1_792_186_862u64 + NTP_TO_UNIX) as u32;
        r[40..44].copy_from_slice(&ntp.to_be_bytes());
        r[44..48].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        r
    }

    #[test]
    fn request_layout() {
        let q = request(NONCE);
        assert_eq!(q[0], 0x23, "LI 0, VN 4, mode 3");
        assert_eq!(&q[40..], &NONCE);
        assert!(q[1..40].iter().all(|&b| b == 0));
    }

    #[test]
    fn parses_valid_reply() {
        let r = parse(&reply(), &request(NONCE)).unwrap();
        assert_eq!(r, Reply { unix_ms: 1_792_186_862_500, stratum: 2 });
    }

    #[test]
    fn rejects_bad_replies() {
        let q = request(NONCE);
        assert_eq!(parse(&reply()[..47], &q), Err(Error::TooShort));
        let mut r = reply();
        r[0] = (4 << 3) | 3;
        assert_eq!(parse(&r, &q), Err(Error::NotServer));
        let mut r = reply();
        r[1] = 0;
        assert_eq!(parse(&r, &q), Err(Error::KissOfDeath));
        let mut r = reply();
        r[0] |= 3 << 6;
        assert_eq!(parse(&r, &q), Err(Error::Unsynchronized));
        assert_eq!(parse(&reply(), &request([9; 8])), Err(Error::OriginMismatch));
        let mut r = reply();
        r[40..44].copy_from_slice(&((1_600_000_000u64 + NTP_TO_UNIX) as u32).to_be_bytes());
        assert_eq!(parse(&r, &q), Err(Error::Implausible), "2020 is before the plausible range");
        let mut r = reply();
        r[40..44].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        assert_eq!(parse(&r, &q), Err(Error::Implausible), "1968, before the Unix epoch: no underflow");
    }

    #[test]
    fn era_rollover() {
        // 2040-01-01 is in NTP era 1.
        let unix = 2_208_988_800u64;
        let mut r = reply();
        r[40..44].copy_from_slice(&((unix + NTP_TO_UNIX - (1 << 32)) as u32).to_be_bytes());
        assert_eq!(parse(&r, &request(NONCE)).unwrap().unix_ms / 1000, unix);
    }

    #[test]
    fn policy() {
        let mut p = SyncPolicy::new();
        assert!(p.due(0));
        p.on_failure(0, None);
        assert!(!p.due(14) && p.due(15));
        p.on_failure(15, None);
        assert!(p.due(45) && !p.due(44), "30 s");
        for _ in 0..10 {
            p.on_failure(100, None);
        }
        assert!(p.due(1000) && !p.due(999), "capped at 15 min");
        p.on_success(2000);
        assert_eq!(p.last_success_s(), Some(2000));
        assert!(!p.due(5599) && p.due(5600));
        p.on_failure(6000, Some(Error::KissOfDeath));
        assert!(!p.due(6899));
    }
}
