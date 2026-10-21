//! Reading request bodies without serde: one small reader for every type, and a short
//! hand-written [`FromJson`] per type that maps keys to fields.
//!
//! serde generates a complete parser per type; for the 14 request types that was about 14 KB of
//! the firmware's 224 KB. This reader is shared and has no generics, so each type costs only its
//! key matching. It accepts what `serde-json-core` accepts for these types (tested against it):
//! unknown keys are skipped, missing optional fields are `None`, a missing required field, a
//! duplicate key, a wrong type or an out-of-range number is an error.

use openkwl_core::num::parse_f32;

/// The body is not what the type expects (the API answers 400 "malformed JSON").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error;

/// The reader's result.
pub type Result<T> = core::result::Result<T, Error>;

/// A request type that can be read from a JSON body.
pub trait FromJson: Sized {
    /// Reads one value from `r` (the object, and nothing after it).
    fn from_json(r: &mut Reader<'_>) -> Result<Self>;
}

/// Reads a whole body: one value of `T`, then nothing but whitespace.
pub fn from_slice<T: FromJson>(body: &[u8]) -> Result<T> {
    let mut r = Reader { b: body, i: 0 };
    let v = T::from_json(&mut r)?;
    r.ws();
    if r.i == r.b.len() { Ok(v) } else { Err(Error) }
}

/// A cursor over a JSON text.
pub struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Which fields an object had: one bit per field, in the order the type lists them.
#[derive(Default)]
pub struct Seen(u32);

impl Seen {
    /// Marks field `bit`; a field given twice is an error (as serde).
    pub fn mark(&mut self, bit: u32) -> Result<()> {
        if self.0 & (1 << bit) != 0 {
            return Err(Error);
        }
        self.0 |= 1 << bit;
        Ok(())
    }

    /// All of the first `n` fields were there.
    pub fn all(&self, n: u32) -> Result<()> {
        if self.0 & ((1 << n) - 1) == (1 << n) - 1 { Ok(()) } else { Err(Error) }
    }
}

impl<'a> Reader<'a> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<()> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(Error)
        }
    }

    fn word(&mut self, w: &[u8]) -> bool {
        self.ws();
        if self.b[self.i..].starts_with(w) {
            self.i += w.len();
            true
        } else {
            false
        }
    }

    /// An object: `field` is called with each key (raw, as written) and reads its value.
    pub fn object(&mut self, field: &mut dyn FnMut(&mut Self, &[u8]) -> Result<()>) -> Result<()> {
        self.expect(b'{')?;
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.expect(b'"')?;
            let start = self.i;
            // Keys are plain names; one with escapes matches no field and is skipped.
            while *self.b.get(self.i).ok_or(Error)? != b'"' {
                self.i += if self.b[self.i] == b'\\' { 2 } else { 1 };
            }
            let key = &self.b[start..self.i];
            self.i += 1;
            self.expect(b':')?;
            field(self, key)?;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(Error),
            }
        }
    }

    /// An array: `item` is called with each index and reads the item.
    pub fn array(&mut self, item: &mut dyn FnMut(&mut Self, usize) -> Result<()>) -> Result<()> {
        self.expect(b'[')?;
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(());
        }
        let mut n = 0;
        loop {
            item(self, n)?;
            n += 1;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(Error),
            }
        }
    }

    /// `null`, consumed: for optional fields.
    pub fn null(&mut self) -> bool {
        self.word(b"null")
    }

    /// `true` or `false`.
    pub fn bool(&mut self) -> Result<bool> {
        if self.word(b"true") {
            Ok(true)
        } else if self.word(b"false") {
            Ok(false)
        } else {
            Err(Error)
        }
    }

    /// A number token as written (`-12.5e3`).
    fn number(&mut self) -> Result<&'a str> {
        self.ws();
        let start = self.i;
        while self.b.get(self.i).is_some_and(|c| matches!(c, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.i += 1;
        }
        let s = core::str::from_utf8(&self.b[start..self.i]).ok().filter(|s| !s.is_empty()).ok_or(Error)?;
        // JSON has no leading zeros (`03`), as serde-json-core refuses them too.
        let digits = s.strip_prefix('-').unwrap_or(s).as_bytes();
        if digits.len() > 1 && digits[0] == b'0' && digits[1].is_ascii_digit() {
            return Err(Error);
        }
        Ok(s)
    }

    /// A whole number in `min..=max` (JSON integers only, as serde for integer fields).
    pub fn int(&mut self, min: i64, max: i64) -> Result<i64> {
        let s = self.number()?;
        let (neg, digits) = match s.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, s),
        };
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error);
        }
        let mut v: i64 = 0;
        for c in digits.bytes() {
            v = v.checked_mul(10).and_then(|v| v.checked_add(i64::from(c - b'0'))).ok_or(Error)?;
        }
        let v = if neg { -v } else { v };
        if (min..=max).contains(&v) { Ok(v) } else { Err(Error) }
    }

    /// A whole number that fits a `u8`.
    pub fn u8(&mut self) -> Result<u8> {
        self.int(0, u8::MAX.into()).map(|v| v as u8)
    }

    /// A whole number that fits a `u16`.
    pub fn u16(&mut self) -> Result<u16> {
        self.int(0, u16::MAX.into()).map(|v| v as u16)
    }

    /// A whole number that fits an `i16`.
    pub fn i16(&mut self) -> Result<i16> {
        self.int(i16::MIN.into(), i16::MAX.into()).map(|v| v as i16)
    }

    /// A whole number that fits a `u32`.
    pub fn u32(&mut self) -> Result<u32> {
        self.int(0, u32::MAX.into()).map(|v| v as u32)
    }

    /// Up to `i64::MAX` (larger values never pass the API's validation anyway).
    pub fn u64(&mut self) -> Result<u64> {
        self.int(0, i64::MAX).map(|v| v as u64)
    }

    /// A finite number.
    pub fn f32(&mut self) -> Result<f32> {
        let s = self.number()?;
        parse_f32(s).filter(|v| v.is_finite()).ok_or(Error)
    }

    /// A string, unescaped into `buf`.
    pub fn str<'b>(&mut self, buf: &'b mut [u8]) -> Result<&'b str> {
        self.expect(b'"')?;
        let mut n = 0;
        loop {
            let c = *self.b.get(self.i).ok_or(Error)?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or(Error)?;
                    self.i += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                // A surrogate pair: `😀`.
                                if !self.word(b"\\u") {
                                    return Err(Error);
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(Error);
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(code).ok_or(Error)?
                        }
                        _ => return Err(Error),
                    };
                    let len = c.len_utf8();
                    c.encode_utf8(buf.get_mut(n..n + len).ok_or(Error)?);
                    n += len;
                }
                // Control characters must be escaped in JSON.
                0..=0x1f => return Err(Error),
                _ => {
                    // UTF-8 as it is: copy the byte.
                    *buf.get_mut(n).ok_or(Error)? = c;
                    n += 1;
                }
            }
        }
        core::str::from_utf8(&buf[..n]).map_err(|_| Error)
    }

    fn hex4(&mut self) -> Result<u32> {
        let h = self.b.get(self.i..self.i + 4).ok_or(Error)?;
        self.i += 4;
        let s = core::str::from_utf8(h).map_err(|_| Error)?;
        u32::from_str_radix(s, 16).map_err(|_| Error)
    }

    /// A string into a bounded string type, e.g. `heapless::String<64>` (too long is an error).
    pub fn string<const N: usize>(&mut self) -> Result<heapless::String<N>> {
        let mut buf = [0u8; N];
        let s = self.str(&mut buf)?;
        heapless::String::try_from(s).map_err(|_| Error)
    }

    /// Any value, ignored: for unknown keys.
    pub fn skip(&mut self) -> Result<()> {
        match self.peek().ok_or(Error)? {
            b'{' => self.object(&mut |r, _| r.skip()),
            b'[' => self.array(&mut |r, _| r.skip()),
            b'"' => {
                self.i += 1;
                loop {
                    match *self.b.get(self.i).ok_or(Error)? {
                        b'"' => break,
                        b'\\' => self.i += 2,
                        _ => self.i += 1,
                    }
                }
                self.i += 1;
                Ok(())
            }
            b't' | b'f' => self.bool().map(|_| ()),
            b'n' => {
                if self.null() {
                    Ok(())
                } else {
                    Err(Error)
                }
            }
            _ => self.f32().map(|_| ()),
        }
    }
}

mod impls;
#[cfg(test)]
mod tests;
