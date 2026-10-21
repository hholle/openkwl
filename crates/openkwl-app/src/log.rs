//! RAM ring buffer behind `GET /api/v1/log` (the SD log is off by default to save writes).

use core::fmt::Write;

pub const CAPACITY: usize = 4096;

#[derive(Debug, Default)]
pub struct Log {
    buf: heapless::Deque<u8, CAPACITY>,
}

impl Log {
    /// Appends `[uptime] message\n`, dropping the oldest lines when full.
    pub fn push(&mut self, uptime_s: u64, args: core::fmt::Arguments<'_>) {
        let mut line: heapless::String<160> = heapless::String::new();
        let _ = write!(line, "[{:>7}s] ", uptime_s);
        let _ = line.write_fmt(args);
        let bytes = line.as_bytes();
        let need = bytes.len() + 1;
        while CAPACITY - self.buf.len() < need {
            // Drop through the end of the oldest line.
            while let Some(b) = self.buf.pop_front() {
                if b == b'\n' {
                    break;
                }
            }
        }
        for &b in bytes {
            let _ = self.buf.push_back(b);
        }
        let _ = self.buf.push_back(b'\n');
    }

    /// Copies the log into `out`, returning the length (truncated to `out`).
    pub fn copy_to(&self, out: &mut [u8]) -> usize {
        let mut n = 0;
        for (dst, src) in out.iter_mut().zip(self.buf.iter()) {
            *dst = *src;
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_whole_recent_lines() {
        let mut log = Log::default();
        for i in 0..1000 {
            log.push(i, format_args!("line {i}"));
        }
        let mut out = [0u8; CAPACITY];
        let n = log.copy_to(&mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.ends_with("line 999\n"));
        assert!(text.starts_with('['), "starts at a line boundary");
        assert!(n <= CAPACITY);
    }
}
