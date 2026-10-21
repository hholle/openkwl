//! Authentication for the device REST API.
//!
//! Replaces the stock scheme (unsalted MD5, one device-wide login state) with:
//! - PBKDF2-HMAC-SHA256 password records with a random per-device salt,
//! - per-client bearer session tokens from the hardware RNG, with idle and absolute expiry,
//! - exponential login throttling.
//!
//! The device speaks plain HTTP; put TLS in front of it (reverse proxy) for anything beyond
//! a trusted LAN, because bearer tokens are otherwise readable on the wire.

#![no_std]

use rand_core::{CryptoRng, RngCore};
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub const SALT_LEN: usize = 16;
pub const HASH_LEN: usize = 32;
pub const TOKEN_LEN: usize = 32;
/// Tuned for a Cortex-M3 at 120 MHz; measure on the target and raise if login stays fast.
pub const DEFAULT_ITERATIONS: u32 = 10_000;
pub const MIN_PASSWORD_LEN: usize = 10;
pub const MAX_PASSWORD_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordError {
    TooShort,
    TooLong,
}

pub fn validate_new_password(password: &str) -> Result<(), PasswordError> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        Err(PasswordError::TooShort)
    } else if password.len() > MAX_PASSWORD_LEN {
        Err(PasswordError::TooLong)
    } else {
        Ok(())
    }
}

/// A stored password: salt, iteration count and derived key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordRecord {
    pub salt: [u8; SALT_LEN],
    pub iterations: u32,
    pub hash: [u8; HASH_LEN],
}

pub const RECORD_LEN: usize = SALT_LEN + 4 + HASH_LEN;

impl PasswordRecord {
    pub fn new<R: RngCore + CryptoRng>(password: &[u8], iterations: u32, rng: &mut R) -> Self {
        let mut salt = [0u8; SALT_LEN];
        rng.fill_bytes(&mut salt);
        Self::with_salt(password, salt, iterations)
    }

    /// [`PasswordRecord::new`] with a salt drawn beforehand, so the slow hashing can run
    /// without holding the random number generator.
    pub fn with_salt(password: &[u8], salt: [u8; SALT_LEN], iterations: u32) -> Self {
        Self { salt, iterations, hash: derive(password, &salt, iterations) }
    }

    pub fn verify(&self, password: &[u8]) -> bool {
        derive(password, &self.salt, self.iterations).ct_eq(&self.hash).into()
    }

    pub fn to_bytes(&self) -> [u8; RECORD_LEN] {
        let mut out = [0u8; RECORD_LEN];
        out[..SALT_LEN].copy_from_slice(&self.salt);
        out[SALT_LEN..SALT_LEN + 4].copy_from_slice(&self.iterations.to_le_bytes());
        out[SALT_LEN + 4..].copy_from_slice(&self.hash);
        out
    }

    pub fn from_bytes(bytes: &[u8; RECORD_LEN]) -> Self {
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[..SALT_LEN]);
        let iterations = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let mut hash = [0u8; HASH_LEN];
        hash.copy_from_slice(&bytes[SALT_LEN + 4..]);
        Self { salt, iterations, hash }
    }
}

fn derive(password: &[u8], salt: &[u8], iterations: u32) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations.max(1), &mut out);
    out
}

/// Opaque bearer token, transported as 64 lowercase hex characters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Token(pub [u8; TOKEN_LEN]);

impl core::fmt::Debug for Token {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl Token {
    pub fn to_hex(&self) -> [u8; TOKEN_LEN * 2] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = [0u8; TOKEN_LEN * 2];
        for (i, b) in self.0.iter().enumerate() {
            out[2 * i] = HEX[usize::from(b >> 4)];
            out[2 * i + 1] = HEX[usize::from(b & 0xf)];
        }
        out
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.as_bytes();
        if s.len() != TOKEN_LEN * 2 {
            return None;
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        };
        let mut out = [0u8; TOKEN_LEN];
        #[allow(clippy::chunks_exact_to_as_chunks)] // keeps the code unchanged; length is checked above
        for (i, pair) in s.chunks_exact(2).enumerate() {
            out[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Some(Token(out))
    }

    /// Parses an `Authorization: Bearer <token>` header value.
    pub fn from_bearer(header: &str) -> Option<Self> {
        let rest = header.strip_prefix("Bearer ")?;
        Self::from_hex(rest.trim())
    }
}

#[derive(Debug, Clone, Copy)]
struct Session {
    token: Token,
    created_s: u64,
    last_used_s: u64,
}

/// Fixed-capacity session table. When full, the least recently used session is evicted.
#[derive(Debug)]
pub struct SessionStore<const N: usize> {
    slots: [Option<Session>; N],
    idle_timeout_s: u64,
    absolute_timeout_s: u64,
}

impl<const N: usize> SessionStore<N> {
    pub const fn new(idle_timeout_s: u64, absolute_timeout_s: u64) -> Self {
        Self { slots: [None; N], idle_timeout_s, absolute_timeout_s }
    }

    pub fn create<R: RngCore + CryptoRng>(&mut self, now_s: u64, rng: &mut R) -> Token {
        self.expire(now_s);
        let mut bytes = [0u8; TOKEN_LEN];
        rng.fill_bytes(&mut bytes);
        let token = Token(bytes);
        let session = Session { token, created_s: now_s, last_used_s: now_s };

        let slot = match self.slots.iter().position(Option::is_none) {
            Some(free) => free,
            None => self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.map_or(0, |s| s.last_used_s))
                .map(|(i, _)| i)
                .unwrap_or(0),
        };
        self.slots[slot] = Some(session);
        token
    }

    /// Checks a token in constant time across all slots and refreshes its idle timer.
    pub fn validate(&mut self, token: &Token, now_s: u64) -> bool {
        self.expire(now_s);
        let mut found = None;
        for (i, slot) in self.slots.iter().enumerate() {
            #[allow(clippy::collapsible_if)] // keeps the constant-time compare visibly separate
            if let Some(s) = slot {
                if bool::from(s.token.0.ct_eq(&token.0)) {
                    found = Some(i);
                }
            }
        }
        match found {
            Some(i) => {
                if let Some(s) = self.slots[i].as_mut() {
                    s.last_used_s = now_s;
                }
                true
            }
            None => false,
        }
    }

    pub fn revoke(&mut self, token: &Token) {
        for slot in self.slots.iter_mut() {
            if slot.is_some_and(|s| bool::from(s.token.0.ct_eq(&token.0))) {
                *slot = None;
            }
        }
    }

    pub fn revoke_all(&mut self) {
        self.slots = [None; N];
    }

    pub fn active(&mut self, now_s: u64) -> usize {
        self.expire(now_s);
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    fn expire(&mut self, now_s: u64) {
        for slot in self.slots.iter_mut() {
            if let Some(s) = slot {
                let idle = now_s.saturating_sub(s.last_used_s) >= self.idle_timeout_s;
                let old = now_s.saturating_sub(s.created_s) >= self.absolute_timeout_s;
                if idle || old {
                    *slot = None;
                }
            }
        }
    }
}

/// Login throttle per client (IPv4 address as `u32`; 0 = unknown): the first
/// `FREE_ATTEMPTS` failures are free, then each further failure doubles that client's lockout,
/// from 5 s up to 15 min. Per client, so someone guessing on the network can't lock the owner
/// out. A ceiling over all clients (`GLOBAL_FAILURES` per `GLOBAL_WINDOW_S`) stops many
/// addresses guessing together, and covers clients pushed out of the small table.
// Seconds as u32 inside (136 years of uptime): 64-bit shifts and compares cost flash on the
// Cortex-M3.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoginThrottle {
    clients: [Client; Self::CLIENTS],
    window_start_s: u32,
    window_failures: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct Client {
    ip: u32,
    failures: u32,
    locked_until_s: u32,
}

impl LoginThrottle {
    pub const FREE_ATTEMPTS: u32 = 3;
    pub const BASE_LOCK_S: u32 = 5;
    pub const MAX_LOCK_S: u32 = 15 * 60;
    /// Clients tracked at once; a new one replaces the one whose lock ended longest ago.
    pub const CLIENTS: usize = 8;
    pub const GLOBAL_FAILURES: u32 = 30;
    pub const GLOBAL_WINDOW_S: u32 = 15 * 60;

    pub const fn new() -> Self {
        Self { clients: [Client { ip: 0, failures: 0, locked_until_s: 0 }; Self::CLIENTS], window_start_s: 0, window_failures: 0 }
    }

    /// `Err(retry_after_s)` while `client` (or everyone, past the ceiling) is locked.
    // Called from several routes: inlined, the table search would be copied into each.
    #[inline(never)]
    pub fn check(&self, client: u32, now_s: u64) -> Result<(), u64> {
        let now = now_s as u32;
        let window_end = self.window_start_s + Self::GLOBAL_WINDOW_S;
        let until = if self.window_failures >= Self::GLOBAL_FAILURES && now < window_end {
            window_end
        } else {
            self.clients.iter().find(|c| c.ip == client && c.failures > 0).map_or(0, |c| c.locked_until_s)
        };
        if now < until { Err(u64::from(until - now)) } else { Ok(()) }
    }

    #[inline(never)]
    pub fn record_failure(&mut self, client: u32, now_s: u64) {
        let now = now_s as u32;
        // The ceiling's window starts with the first failure after the last one ended.
        if self.window_failures == 0 || now >= self.window_start_s + Self::GLOBAL_WINDOW_S {
            self.window_start_s = now;
            self.window_failures = 0;
        }
        self.window_failures = self.window_failures.saturating_add(1);
        let i = match self.clients.iter().position(|c| c.ip == client && c.failures > 0) {
            Some(i) => i,
            // A free slot, else the one whose lock ended (or ends) first.
            None => {
                let mut i = 0;
                for (j, c) in self.clients.iter().enumerate() {
                    let best = &self.clients[i];
                    let free = (c.failures == 0, best.failures == 0);
                    if free == (true, false) || (free.0 == free.1 && c.locked_until_s < best.locked_until_s) {
                        i = j;
                    }
                }
                self.clients[i] = Client { ip: client, failures: 0, locked_until_s: 0 };
                i
            }
        };
        let c = &mut self.clients[i];
        c.failures = c.failures.saturating_add(1);
        if c.failures > Self::FREE_ATTEMPTS {
            let exp = (c.failures - Self::FREE_ATTEMPTS - 1).min(8);
            c.locked_until_s = now + (Self::BASE_LOCK_S << exp).min(Self::MAX_LOCK_S);
        }
    }

    /// The right password from `client`: its count starts over (the ceiling's window doesn't).
    pub fn record_success(&mut self, client: u32) {
        for c in self.clients.iter_mut().filter(|c| c.ip == client) {
            *c = Client::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic RNG for tests only.
    struct TestRng(u64);
    impl RngCore for TestRng {
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for b in dest {
                *b = self.next_u64() as u8;
            }
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl CryptoRng for TestRng {}

    #[test]
    fn password_record() {
        let mut rng = TestRng(42);
        let rec = PasswordRecord::new(b"correct horse battery", 1000, &mut rng);
        assert!(rec.verify(b"correct horse battery"));
        assert!(!rec.verify(b"wrong password!!"));
        assert_eq!(PasswordRecord::from_bytes(&rec.to_bytes()), rec);
        let other = PasswordRecord::new(b"correct horse battery", 1000, &mut rng);
        assert_ne!(rec.hash, other.hash, "salted");
    }

    #[test]
    fn known_pbkdf2_vector() {
        // RFC 7914 §11 PBKDF2-HMAC-SHA256 test vector ("passwd", "salt", c = 1).
        let out = derive(b"passwd", b"salt", 1);
        assert_eq!(out[..4], [0x55, 0xac, 0x04, 0x6e]);
    }

    #[test]
    fn password_policy() {
        assert_eq!(validate_new_password("short"), Err(PasswordError::TooShort));
        assert!(validate_new_password("long enough pw").is_ok());
    }

    #[test]
    fn token_hex() {
        let t = Token([0xab; TOKEN_LEN]);
        let hex = t.to_hex();
        let s = core::str::from_utf8(&hex).unwrap();
        assert_eq!(Token::from_hex(s), Some(t));
        let mut header = [0u8; 7 + 64];
        header[..7].copy_from_slice(b"Bearer ");
        header[7..].copy_from_slice(&hex);
        assert_eq!(Token::from_bearer(core::str::from_utf8(&header).unwrap()), Some(t));
        assert_eq!(Token::from_hex("zz"), None);
    }

    #[test]
    fn sessions_expire_and_evict() {
        let mut rng = TestRng(7);
        let mut store = SessionStore::<2>::new(600, 3600);
        let a = store.create(0, &mut rng);
        assert!(store.validate(&a, 100));
        assert!(!store.validate(&a, 800), "idle timeout");

        let b = store.create(1000, &mut rng);
        let c = store.create(1001, &mut rng);
        store.validate(&b, 1002);
        let d = store.create(1003, &mut rng);
        assert!(store.validate(&b, 1004), "recently used session kept");
        assert!(!store.validate(&c, 1004), "least recently used evicted");
        assert!(store.validate(&d, 1004));

        store.revoke(&d);
        assert!(!store.validate(&d, 1005));
        assert!(!store.validate(&b, 1000 + 3600), "absolute timeout");
    }

    #[test]
    fn throttle_backs_off() {
        let mut t = LoginThrottle::new();
        const ME: u32 = 0xC0A8_0132;
        for _ in 0..3 {
            t.record_failure(ME, 0);
            assert!(t.check(ME, 0).is_ok());
        }
        t.record_failure(ME, 0);
        assert_eq!(t.check(ME, 0), Err(5));
        t.record_failure(ME, 10);
        assert_eq!(t.check(ME, 10), Err(10));
        for _ in 0..20 {
            t.record_failure(ME, 100);
        }
        assert_eq!(t.check(ME, 100), Err(u64::from(LoginThrottle::MAX_LOCK_S)));
        t.record_success(ME);
        assert!(t.check(ME, 100).is_ok());
    }

    #[test]
    fn throttle_locks_only_the_guessing_client() {
        let (owner, guesser) = (0xC0A8_0102, 0xC0A8_0163);
        let mut t = LoginThrottle::new();
        for _ in 0..10 {
            t.record_failure(guesser, 50);
        }
        assert!(t.check(guesser, 50).is_err());
        assert!(t.check(owner, 50).is_ok(), "the owner can still log in");
    }

    #[test]
    fn throttle_ceiling_over_all_clients() {
        let mut t = LoginThrottle::new();
        // Many addresses, three free guesses each: the ceiling stops them together.
        for i in 0..LoginThrottle::GLOBAL_FAILURES {
            t.record_failure(0x0A00_0000 + i / 3, 100);
        }
        assert_eq!(t.check(0x0A00_FFFF, 100), Err(u64::from(LoginThrottle::GLOBAL_WINDOW_S)));
        assert!(t.check(0x0A00_FFFF, 100 + u64::from(LoginThrottle::GLOBAL_WINDOW_S)).is_ok(), "window over");
    }

    #[test]
    fn throttle_table_keeps_locked_clients() {
        let mut t = LoginThrottle::new();
        let locked = 0xC0A8_01FE;
        for _ in 0..6 {
            t.record_failure(locked, 0);
        }
        // Seven more clients with one failure each fill the table; the locked one stays.
        for i in 0..7 {
            t.record_failure(0xC0A8_0200 + i, 1);
        }
        assert!(t.check(locked, 2).is_err());
        // An eighth newcomer replaces a client without a running lock, not the locked one.
        t.record_failure(0xC0A8_0300, 2);
        assert!(t.check(locked, 3).is_err());
    }
}
