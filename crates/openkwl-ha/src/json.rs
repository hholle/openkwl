//! A small JSON object writer for the state and discovery documents.
//!
//! `#[derive(Serialize)]` on a struct with 30 fields inlines one serializer call per field
//! into every caller; here each value goes through a handful of shared, non-generic calls,
//! with strings and numbers still formatted by `serde-json-core` (same escaping and float
//! output as before).

use serde::Serialize;

use crate::Error;

pub(crate) struct Obj<'b> {
    buf: &'b mut [u8],
    len: usize,
    /// No member written yet at the current nesting level.
    first: bool,
    full: bool,
}

impl<'b> Obj<'b> {
    pub(crate) fn new(buf: &'b mut [u8]) -> Self {
        let mut o = Obj { buf, len: 0, first: true, full: false };
        o.raw(b"{");
        o
    }

    fn raw(&mut self, s: &[u8]) {
        match self.buf.get_mut(self.len..self.len + s.len()) {
            Some(dst) => {
                dst.copy_from_slice(s);
                self.len += s.len();
            }
            None => self.full = true,
        }
    }

    /// `"k":` with the separating comma.
    #[inline(never)]
    fn key(&mut self, k: &str) {
        if !self.first {
            self.raw(b",");
        }
        self.first = false;
        self.raw(b"\"");
        self.raw(k.as_bytes());
        self.raw(b"\":");
    }

    /// Any value through `serde-json-core`, written in place.
    fn value<T: Serialize + ?Sized>(&mut self, v: &T) {
        match serde_json_core::to_slice(v, self.buf.get_mut(self.len..).unwrap_or_default()) {
            Ok(n) => self.len += n,
            Err(_) => self.full = true,
        }
    }

    #[inline(never)]
    pub(crate) fn str(&mut self, k: &str, v: &str) {
        self.key(k);
        self.value(v);
    }

    /// Left out when `None`.
    pub(crate) fn opt_str(&mut self, k: &str, v: Option<&str>) {
        if let Some(v) = v {
            self.str(k, v);
        }
    }

    /// `null` when `None`.
    #[inline(never)]
    pub(crate) fn str_or_null(&mut self, k: &str, v: Option<&str>) {
        match v {
            Some(v) => self.str(k, v),
            None => self.null(k),
        }
    }

    #[inline(never)]
    pub(crate) fn null(&mut self, k: &str) {
        self.key(k);
        self.raw(b"null");
    }

    /// `null` when `None`.
    #[inline(never)]
    pub(crate) fn f32(&mut self, k: &str, v: Option<f32>) {
        self.key(k);
        self.value(&v);
    }

    #[inline(never)]
    pub(crate) fn u32(&mut self, k: &str, v: u32) {
        self.key(k);
        self.value(&v);
    }

    /// `null` when `None`.
    #[inline(never)]
    pub(crate) fn opt_u32(&mut self, k: &str, v: Option<u32>) {
        self.key(k);
        self.value(&v);
    }

    /// An enum or list with its own `Serialize`.
    pub(crate) fn any<T: Serialize + ?Sized>(&mut self, k: &str, v: &T) {
        self.key(k);
        self.value(v);
    }

    /// `"k":["a","b",…]`.
    pub(crate) fn strs(&mut self, k: &str, v: &[&str]) {
        self.key(k);
        self.list(v);
    }

    #[inline(never)]
    fn list(&mut self, v: &[&str]) {
        self.raw(b"[");
        for (i, s) in v.iter().enumerate() {
            if i > 0 {
                self.raw(b",");
            }
            self.value(*s);
        }
        self.raw(b"]");
    }

    /// Starts a list of lists under `k`: [`Obj::strs_item`] for each, then [`Obj::close_list`].
    pub(crate) fn open_list(&mut self, k: &str) {
        self.key(k);
        self.raw(b"[");
        self.first = true;
    }

    pub(crate) fn strs_item(&mut self, v: &[&str]) {
        if !self.first {
            self.raw(b",");
        }
        self.first = false;
        self.list(v);
    }

    pub(crate) fn close_list(&mut self) {
        self.raw(b"]");
        self.first = false;
    }

    /// Starts a nested object under `k`; end it with [`Obj::close`].
    pub(crate) fn open(&mut self, k: &str) {
        self.key(k);
        self.raw(b"{");
        self.first = true;
    }

    pub(crate) fn close(&mut self) {
        self.raw(b"}");
        self.first = false;
    }

    /// Closes the outer object; the document's length.
    pub(crate) fn finish(mut self) -> Result<usize, Error> {
        self.raw(b"}");
        if self.full { Err(Error::TooLong) } else { Ok(self.len) }
    }
}
