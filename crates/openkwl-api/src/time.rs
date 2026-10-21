//! Date and time values as readable JSON strings, backed by [`openkwl_core::clock`] types.
//!
//! | Type | JSON | Example |
//! |---|---|---|
//! | [`TimeOfDay`] | `"HH:MM"` | `"06:30"` |
//! | [`MonthDay`] | `"MM-DD"` | `"03-01"` |
//! | [`Date`] | `"YYYY-MM-DD"` | `"2024-07-13"` |
//! | [`DateTime`] | `"YYYY-MM-DDTHH:MM:SS"` (local, no offset) | `"2026-10-16T23:41:02"` |

use core::fmt::Write as _;

use openkwl_core::clock;
use serde::{Deserialize, Serialize};

fn digits(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn render<const N: usize>(args: core::fmt::Arguments<'_>) -> heapless::String<N> {
    let mut s = heapless::String::new();
    // Every caller formats a fixed-width value that fits N.
    let _ = s.write_fmt(args);
    s
}

/// Minutes since midnight, `"HH:MM"` (00:00–23:59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "heapless::String<5>", into = "heapless::String<5>")]
pub struct TimeOfDay(pub u16);

impl TimeOfDay {
    /// Validated constructor.
    pub const fn new(hour: u16, minute: u16) -> Option<Self> {
        if hour < 24 && minute < 60 { Some(Self(hour * 60 + minute)) } else { None }
    }
}

impl TryFrom<heapless::String<5>> for TimeOfDay {
    type Error = &'static str;
    fn try_from(s: heapless::String<5>) -> Result<Self, Self::Error> {
        let (h, m) = s.split_once(':').filter(|(h, m)| h.len() == 2 && m.len() == 2).ok_or("expected HH:MM")?;
        Self::new(digits(h).ok_or("expected HH:MM")?, digits(m).ok_or("expected HH:MM")?).ok_or("time out of range")
    }
}

impl From<TimeOfDay> for heapless::String<5> {
    fn from(t: TimeOfDay) -> Self {
        render(format_args!("{:02}:{:02}", t.0 / 60, t.0 % 60))
    }
}

/// Day of the year without year, `"MM-DD"`; 29 February is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "heapless::String<5>", into = "heapless::String<5>")]
pub struct MonthDay(pub clock::MonthDay);

impl TryFrom<heapless::String<5>> for MonthDay {
    type Error = &'static str;
    fn try_from(s: heapless::String<5>) -> Result<Self, Self::Error> {
        let (m, d) = s.split_once('-').filter(|(m, d)| m.len() == 2 && d.len() == 2).ok_or("expected MM-DD")?;
        let (m, d) = (digits(m).ok_or("expected MM-DD")?, digits(d).ok_or("expected MM-DD")?);
        clock::MonthDay::new(m as u8, d as u8).filter(|_| m <= 12 && d <= 31).map(Self).ok_or("no such day")
    }
}

impl From<MonthDay> for heapless::String<5> {
    fn from(md: MonthDay) -> Self {
        render(format_args!("{:02}-{:02}", md.0.month, md.0.day))
    }
}

/// Calendar date, `"YYYY-MM-DD"` (1970–2099).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "heapless::String<10>", into = "heapless::String<10>")]
pub struct Date(pub clock::Date);

fn parse_date(s: &str) -> Option<clock::Date> {
    let mut it = s.splitn(3, '-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (m, d) = (digits(m)?, digits(d)?);
    if m > 12 || d > 31 {
        return None;
    }
    clock::Date::new(digits(y)?, m as u8, d as u8)
}

impl TryFrom<heapless::String<10>> for Date {
    type Error = &'static str;
    fn try_from(s: heapless::String<10>) -> Result<Self, Self::Error> {
        parse_date(&s).map(Self).ok_or("expected YYYY-MM-DD (1970-2099)")
    }
}

impl From<Date> for heapless::String<10> {
    fn from(d: Date) -> Self {
        render(format_args!("{:04}-{:02}-{:02}", d.0.year, d.0.month, d.0.day))
    }
}

/// Local date and time, `"YYYY-MM-DDTHH:MM:SS"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "heapless::String<19>", into = "heapless::String<19>")]
pub struct DateTime {
    /// Local date.
    pub date: clock::Date,
    /// Seconds since local midnight.
    pub second_of_day: u32,
}

impl From<clock::LocalDateTime> for DateTime {
    fn from(l: clock::LocalDateTime) -> Self {
        Self { date: l.date, second_of_day: u32::from(l.minute) * 60 + u32::from(l.second) }
    }
}

impl TryFrom<heapless::String<19>> for DateTime {
    type Error = &'static str;
    fn try_from(s: heapless::String<19>) -> Result<Self, Self::Error> {
        const ERR: &str = "expected YYYY-MM-DDTHH:MM:SS";
        let (d, t) = s.split_once('T').ok_or(ERR)?;
        let date = parse_date(d).ok_or(ERR)?;
        let mut it = t.splitn(3, ':');
        let mut part = |max: u16| it.next().filter(|p| p.len() == 2).and_then(digits).filter(|v| *v < max);
        let (h, m, sec) = (part(24).ok_or(ERR)?, part(60).ok_or(ERR)?, part(60).ok_or(ERR)?);
        Ok(Self { date, second_of_day: u32::from(h) * 3600 + u32::from(m) * 60 + u32::from(sec) })
    }
}

impl From<DateTime> for heapless::String<19> {
    fn from(t: DateTime) -> Self {
        let s = t.second_of_day;
        render(format_args!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            t.date.year,
            t.date.month,
            t.date.day,
            s / 3600,
            s / 60 % 60,
            s % 60
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{parse, round_trip};

    #[test]
    fn time_of_day() {
        assert_eq!(round_trip::<_, 16>(&TimeOfDay(6 * 60 + 30)), "\"06:30\"");
        assert_eq!(round_trip::<_, 16>(&TimeOfDay(0)), "\"00:00\"");
        for bad in ["\"24:00\"", "\"6:30\"", "\"06:60\"", "\"06-30\"", "\"0a:00\"", "\"+6:30\""] {
            assert!(parse::<TimeOfDay>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn month_day() {
        let md = parse::<MonthDay>("\"02-29\"").unwrap();
        assert_eq!(md.0, clock::MonthDay::new(2, 29).unwrap());
        assert_eq!(round_trip::<_, 16>(&md), "\"02-29\"");
        for bad in ["\"02-30\"", "\"13-01\"", "\"1-01\"", "\"00-10\""] {
            assert!(parse::<MonthDay>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn date() {
        let d = parse::<Date>("\"2024-07-13\"").unwrap();
        assert_eq!(d.0, clock::Date::new(2024, 7, 13).unwrap());
        assert_eq!(round_trip::<_, 16>(&d), "\"2024-07-13\"");
        for bad in ["\"2026-02-29\"", "\"2100-01-01\"", "\"24-07-13\"", "\"2024-7-13\"", "\"2024-07-13x\""] {
            assert!(parse::<Date>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn date_time() {
        let l = clock::TimeZone::CET.to_local(1_792_186_862); // 2026-10-16T21:41:02Z
        let dt = DateTime::from(l);
        assert_eq!(round_trip::<_, 32>(&dt), "\"2026-10-16T23:41:02\"");
        assert!(parse::<DateTime>("\"2026-10-16T24:00:00\"").is_err());
        assert!(parse::<DateTime>("\"2026-10-16 23:41:02\"").is_err());
    }
}
