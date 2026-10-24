//! The day record: one local day in quarter-hour points plus daily totals, kept in RAM while
//! the day runs and written to the SD card once after midnight (the only card write for the
//! history). Served as it is (`GET /api/v1/history/today`, `/history/days/…`); the web UI
//! decodes it and computes the statistics.
//!
//! Layout, little-endian, [`RECORD_LEN`] bytes:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | 0 | 2 × u8 | `"KL"` |
//! | 2 | u8 | version, [`VERSION`] |
//! | 3 | u8 | minutes per point, 15 |
//! | 4 | u16 | local date, days since 1970-01-01 |
//! | 6 | u16 | reserved |
//! | 8 | u32 | CRC-32 of bytes 12.. |
//! | 12 | u32 | seconds recorded |
//! | 16 | 5 × u32 | seconds at level 0 (off) to 4 |
//! | 36 | u32 | seconds bypass open |
//! | 40 | u32 | seconds frost protection |
//! | 44 | u32 | heat recovered, Wh |
//! | 48 | u32 | cooling recovered, Wh |
//! | 52 | u16 | mean supply fan rpm while running |
//! | 54 | u16 | mean exhaust fan rpm while running |
//! | 56 | 8 × u8 | reserved |
//! | 64 + 12 i | point | quarter-hour `i` (0–95) of the local day |
//!
//! A point: outdoor, supply, extract, exhaust air as i16 in 0.1 °C and the recovered heat power
//! as i16 in W (each [`NONE`] without a value), the level as u8 ([`NOT_RECORDED`] for a quarter
//! without data) and flags as u8 (bit 0 bypass open, bit 1 frost protection), all at the end
//! of the quarter except the means.

use openkwl_config::crc::crc32;
use openkwl_core::clock::LocalDateTime;

pub const VERSION: u8 = 1;
pub const POINTS: usize = 96;
const POINT_LEN: usize = 12;
const HEADER_LEN: usize = 64;
pub const RECORD_LEN: usize = HEADER_LEN + POINTS * POINT_LEN;
/// Place of a day's record in `HELIOS/DAYS.BIN`: `(day % SLOTS) * SLOT_LEN`.
pub const SLOTS: u32 = 366;
pub const SLOT_LEN: u32 = 2048;
/// "No value" for a temperature or the power.
pub const NONE: i16 = i16::MIN;
pub const NOT_RECORDED: u8 = 255;
const MAGIC: [u8; 2] = *b"KL";
const SECONDS_PER_POINT: u32 = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    values: [i16; 5],
    level: u8,
    flags: u8,
}

const EMPTY: Point = Point { values: [NONE; 5], level: NOT_RECORDED, flags: 0 };

/// What one second contributes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Outdoor, supply, extract, exhaust air in °C.
    pub temperatures: [Option<f32>; 4],
    /// Recovered heat power in W (negative = cooling).
    pub power_w: Option<f32>,
    pub level: u8,
    pub bypass_open: bool,
    pub frost: bool,
    pub supply_rpm: u32,
    pub exhaust_rpm: u32,
}

/// Running sums of the quarter in progress.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Quarter {
    sums: [f32; 5],
    counts: [u16; 5],
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Totals {
    recorded_s: u32,
    level_s: [u32; 5],
    bypass_s: u32,
    frost_s: u32,
    heating_wh: f32,
    cooling_wh: f32,
    running_s: u32,
    supply_rpm_sum: u32,
    exhaust_rpm_sum: u32,
}

/// Today's record, and after midnight the finished day until it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorder {
    /// The day being recorded (days since 1970-01-01).
    day: Option<u16>,
    quarter: Option<u8>,
    acc: Quarter,
    totals: Totals,
    points: [Point; POINTS],
    /// The day ended and waits to be written (`take_finished`); uptime when it ended.
    finished_at: Option<u64>,
    /// The write was requested and not finished yet.
    requested: bool,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub const fn new() -> Self {
        Self {
            day: None,
            quarter: None,
            acc: Quarter { sums: [0.0; 5], counts: [0; 5] },
            totals: Totals {
                recorded_s: 0,
                level_s: [0; 5],
                bypass_s: 0,
                frost_s: 0,
                heating_wh: 0.0,
                cooling_wh: 0.0,
                running_s: 0,
                supply_rpm_sum: 0,
                exhaust_rpm_sum: 0,
            },
            points: [EMPTY; POINTS],
            finished_at: None,
            requested: false,
        }
    }

    /// One second of the control loop; nothing is recorded while the clock is not set.
    pub fn on_second(&mut self, now: Option<LocalDateTime>, uptime_s: u64, s: &Sample) {
        let Some(t) = now else { return };
        let day = t.date.days() as u16;
        let quarter = (t.minute / 15) as u8;
        if let Some(ended) = self.finished_at {
            // The finished day waits for its write; given up after a quarter (no card).
            if uptime_s.saturating_sub(ended) < u64::from(SECONDS_PER_POINT) {
                return;
            }
            *self = Self::new();
        }
        if self.day != Some(day) {
            if self.day.is_some() {
                // Midnight (or the clock moved to another day): keep the day for its write.
                self.close_quarter();
                self.finished_at = Some(uptime_s);
                return;
            }
            self.day = Some(day);
        }
        if self.quarter != Some(quarter) {
            self.close_quarter();
            self.quarter = Some(quarter);
        }
        self.add(s);
    }

    fn add(&mut self, s: &Sample) {
        let values = [s.temperatures[0], s.temperatures[1], s.temperatures[2], s.temperatures[3], s.power_w];
        for (i, v) in values.into_iter().enumerate() {
            if let Some(v) = v.filter(|v| v.is_finite()) {
                self.acc.sums[i] += v;
                self.acc.counts[i] += 1;
            }
        }
        let t = &mut self.totals;
        t.recorded_s += 1;
        t.level_s[usize::from(s.level.min(4))] += 1;
        t.bypass_s += u32::from(s.bypass_open);
        t.frost_s += u32::from(s.frost);
        match s.power_w {
            Some(p) if p > 0.0 => t.heating_wh += p / 3600.0,
            Some(p) if p < 0.0 => t.cooling_wh -= p / 3600.0,
            _ => {}
        }
        if s.supply_rpm > 0 || s.exhaust_rpm > 0 {
            t.running_s += 1;
            t.supply_rpm_sum = t.supply_rpm_sum.saturating_add(s.supply_rpm);
            t.exhaust_rpm_sum = t.exhaust_rpm_sum.saturating_add(s.exhaust_rpm);
        }
        let i = usize::from(self.quarter.unwrap_or(0));
        self.points[i].level = s.level;
        self.points[i].flags = u8::from(s.bypass_open) | u8::from(s.frost) << 1;
    }

    /// Stores the means of the quarter in progress in its point.
    fn close_quarter(&mut self) {
        if let Some(q) = self.quarter.take() {
            let p = &mut self.points[usize::from(q)];
            for i in 0..5 {
                let n = self.acc.counts[i];
                p.values[i] = if n == 0 { NONE } else { scaled(self.acc.sums[i] / f32::from(n), i) };
            }
        }
        self.acc = Quarter::default();
    }

    /// A finished day to write now (once; `None` while there is none or it was handed out).
    pub fn take_finished(&mut self) -> bool {
        let due = self.finished_at.is_some() && !self.requested;
        self.requested |= due;
        due
    }

    /// The finished day's slot in `DAYS.BIN` and its record; then starts the new day. `None`
    /// when no day is finished.
    pub fn finished_record(&mut self, out: &mut [u8]) -> Option<(u32, usize)> {
        self.finished_at?;
        let day = self.day?;
        let len = self.encode(out);
        *self = Self::new();
        Some((slot_offset(day), len))
    }

    /// The record of the day in progress (the quarter in progress is not in it yet), or `None`
    /// while the clock is not set. Returns its length.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        let Some(day) = self.day else { return 0 };
        let out = &mut out[..RECORD_LEN];
        out.fill(0);
        let t = &self.totals;
        out[..2].copy_from_slice(&MAGIC);
        out[2] = VERSION;
        out[3] = (SECONDS_PER_POINT / 60) as u8;
        out[4..6].copy_from_slice(&day.to_le_bytes());
        let mut put = |at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(12, t.recorded_s);
        for (i, s) in t.level_s.iter().enumerate() {
            put(16 + 4 * i, *s);
        }
        put(36, t.bypass_s);
        put(40, t.frost_s);
        put(44, t.heating_wh as u32);
        put(48, t.cooling_wh as u32);
        let mean = |sum: u32| sum.checked_div(t.running_s).unwrap_or(0).min(u32::from(u16::MAX)) as u16;
        out[52..54].copy_from_slice(&mean(t.supply_rpm_sum).to_le_bytes());
        out[54..56].copy_from_slice(&mean(t.exhaust_rpm_sum).to_le_bytes());
        for (i, p) in self.points.iter().enumerate() {
            let at = HEADER_LEN + i * POINT_LEN;
            for (k, v) in p.values.iter().enumerate() {
                out[at + 2 * k..at + 2 * k + 2].copy_from_slice(&v.to_le_bytes());
            }
            // The quarter in progress has no means yet.
            let open = self.quarter == Some(i as u8) && self.finished_at.is_none();
            out[at + 10] = if open { NOT_RECORDED } else { p.level };
            out[at + 11] = p.flags;
        }
        let crc = crc32(&out[12..]);
        out[8..12].copy_from_slice(&crc.to_le_bytes());
        RECORD_LEN
    }
}

/// Temperatures in 0.1 °C, the power in W, clamped to what i16 holds (NONE excluded).
fn scaled(v: f32, i: usize) -> i16 {
    let v = if i < 4 { v * 10.0 } else { v };
    let r = if v < 0.0 { v - 0.5 } else { v + 0.5 };
    r.clamp(f32::from(NONE + 1), f32::from(i16::MAX)) as i16
}

/// The days a `GET /api/v1/history/days/{arg}` asks for: `YYYY-MM-DD` one day, `YYYY-MM`
/// every day of the month. Returns the first day (days since 1970-01-01) and how many.
pub fn days_requested(arg: &str) -> Option<(u16, u8)> {
    let num = |s: &str| s.bytes().try_fold(0u16, |n, b| b.is_ascii_digit().then(|| n * 10 + u16::from(b - b'0')));
    let (date, count) = match arg.len() {
        7 => {
            let (y, m) = (num(arg.get(..4)?)?, num(arg.get(5..)?)?);
            (openkwl_core::clock::Date::new(y, m as u8, 1)?, openkwl_core::clock::days_in_month(y, m as u8))
        }
        10 => (openkwl_core::clock::Date::new(num(arg.get(..4)?)?, num(arg.get(5..7)?)? as u8, num(arg.get(8..)?)? as u8)?, 1),
        _ => return None,
    };
    (arg.as_bytes()[4] == b'-' && (count != 1 || arg.as_bytes()[7] == b'-')).then_some((date.days() as u16, count))
}

/// Where `day`'s record is in `HELIOS/DAYS.BIN`.
pub fn slot_offset(day: u16) -> u32 {
    u32::from(day) % SLOTS * SLOT_LEN
}

/// Whether `record` is a valid record of `day`.
pub fn valid(record: &[u8], day: u16) -> bool {
    record.len() >= RECORD_LEN
        && record[..2] == MAGIC
        && record[2] == VERSION
        && record[4..6] == day.to_le_bytes()
        && record[8..12] == crc32(&record[12..RECORD_LEN]).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use openkwl_core::clock::Date;

    use super::*;

    fn at(day: u8, minute: u16, second: u8) -> Option<LocalDateTime> {
        Some(LocalDateTime { date: Date::new(2026, 10, day).unwrap(), minute, second, dst: false })
    }

    fn sample(outdoor: f32, level: u8) -> Sample {
        Sample {
            temperatures: [Some(outdoor), Some(19.0), Some(21.5), None],
            power_w: Some(360.0),
            level,
            bypass_open: false,
            frost: level == 1,
            supply_rpm: 1000,
            exhaust_rpm: 1100,
        }
    }

    fn i16_at(r: &[u8], at: usize) -> i16 {
        i16::from_le_bytes([r[at], r[at + 1]])
    }

    fn u32_at(r: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn requested_days() {
        let oct1 = Date::new(2026, 10, 1).unwrap().days() as u16;
        assert_eq!(days_requested("2026-10"), Some((oct1, 31)));
        assert_eq!(days_requested("2026-10-24"), Some((oct1 + 23, 1)));
        assert_eq!(days_requested("2028-02").map(|d| d.1), Some(29));
        for bad in ["2026-13", "2026-10-32", "2026/10", "2026-1-24", "+026-10", "2026-10-2x", "", "2026-10-24-1"] {
            assert_eq!(days_requested(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_without_a_clock() {
        let mut r = Recorder::new();
        r.on_second(None, 1, &sample(5.0, 2));
        assert_eq!(r.encode(&mut [0; RECORD_LEN]), 0);
    }

    #[test]
    fn quarters_hold_means_and_the_day_its_totals() {
        let mut r = Recorder::new();
        let mut up = 0;
        // 00:00–00:15 at 4 °C and 6 °C (mean 5.0), then 00:15–00:30 at -2.25 °C.
        for s in 0..900u32 {
            up += 1;
            let t = if s < 450 { 4.0 } else { 6.0 };
            r.on_second(at(17, (s / 60) as u16, (s % 60) as u8), up, &sample(t, 2));
        }
        for s in 0..900u32 {
            up += 1;
            r.on_second(at(17, 15 + (s / 60) as u16, (s % 60) as u8), up, &sample(-2.25, 3));
        }
        let mut out = [0u8; RECORD_LEN];
        assert_eq!(r.encode(&mut out), RECORD_LEN);
        let day = Date::new(2026, 10, 17).unwrap().days() as u16;
        assert!(valid(&out, day));
        assert!(!valid(&out, day + 1));
        assert_eq!(i16_at(&out, HEADER_LEN), 50, "quarter 0 outdoor 5.0 °C");
        assert_eq!(i16_at(&out, HEADER_LEN + 2), 190);
        assert_eq!(i16_at(&out, HEADER_LEN + 6), NONE, "no exhaust reading");
        assert_eq!(i16_at(&out, HEADER_LEN + 8), 360, "power");
        assert_eq!(out[HEADER_LEN + 10], 2);
        assert_eq!(out[HEADER_LEN + POINT_LEN + 10], NOT_RECORDED, "quarter 1 still in progress");
        assert_eq!(out[HEADER_LEN + 2 * POINT_LEN + 10], NOT_RECORDED, "quarter 2 not reached");
        assert_eq!(u32_at(&out, 12), 1800);
        assert_eq!((u32_at(&out, 16 + 8), u32_at(&out, 16 + 12)), (900, 900), "seconds at level 2 and 3");
        assert_eq!(u32_at(&out, 44), 180, "360 W for half an hour");
        assert_eq!(u16::from_le_bytes([out[52], out[53]]), 1000);

        // Midnight: the day is kept until it is written, then the new day starts.
        up += 1;
        r.on_second(at(18, 0, 0), up, &sample(0.0, 1));
        assert!(r.take_finished());
        assert!(!r.take_finished(), "handed out once");
        let (offset, len) = r.finished_record(&mut out).unwrap();
        assert_eq!((offset, len), (u32::from(day) % SLOTS * SLOT_LEN, RECORD_LEN));
        assert!(valid(&out, day));
        assert_eq!(i16_at(&out, HEADER_LEN + POINT_LEN), -23, "quarter 1 closed at midnight: -2.25 → -2.3 °C");
        assert_eq!(out[HEADER_LEN + POINT_LEN + 10], 3);
        up += 1;
        r.on_second(at(18, 0, 1), up, &sample(0.0, 1));
        r.encode(&mut out);
        assert!(valid(&out, day + 1));
        assert_eq!(u32_at(&out, 12), 1);
    }

    #[test]
    fn an_unwritten_day_is_dropped_after_a_quarter() {
        let mut r = Recorder::new();
        r.on_second(at(17, 1439, 59), 1, &sample(1.0, 1));
        r.on_second(at(18, 0, 0), 2, &sample(1.0, 1));
        assert!(r.take_finished());
        r.on_second(at(18, 10, 0), 600, &sample(1.0, 1));
        assert_eq!(r.encode(&mut [0; RECORD_LEN]), RECORD_LEN, "still the old day");
        r.on_second(at(18, 15, 0), 903, &sample(1.0, 1));
        let mut out = [0u8; RECORD_LEN];
        r.encode(&mut out);
        assert!(valid(&out, Date::new(2026, 10, 18).unwrap().days() as u16));
        assert_eq!(r.finished_record(&mut out), None);
    }
}
