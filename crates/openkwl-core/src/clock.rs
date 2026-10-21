//! Calendar and local time without a time-zone database.
//!
//! The firmware keeps UTC (RTC set by SNTP) and converts to local time with a fixed offset
//! and, optionally, the EU daylight-saving rule (last Sunday of March to last Sunday of
//! October, switching at 01:00 UTC). That covers this unit's location (stock country `DE`).

/// Days of the week, Monday first like the stock weekly program.
pub const WEEKDAYS: usize = 7;
pub const MINUTES_PER_DAY: u16 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

const fn is_leap(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

pub const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// Valid dates from 1970 to 2099.
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year < 1970 || year > 2099 || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    pub const fn days(self) -> u32 {
        let y = if self.month <= 2 { self.year as u32 - 1 } else { self.year as u32 };
        let era = y / 400;
        let yoe = y - era * 400;
        let m = self.month as u32;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as u32 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Inverse of [`Date::days`].
    pub const fn from_days(days: u32) -> Self {
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
        Self { year, month, day }
    }

    /// 0 = Monday … 6 = Sunday.
    pub const fn weekday(self) -> usize {
        // 1970-01-01 was a Thursday.
        ((self.days() + 3) % 7) as usize
    }

    pub const fn month_day(self) -> MonthDay {
        MonthDay { month: self.month, day: self.day }
    }

    /// Last Sunday of `month`.
    pub const fn last_sunday(year: u16, month: u8) -> Self {
        let last = Self { year, month, day: days_in_month(year, month) };
        let back = (last.weekday() + 1) % 7;
        Self { year, month, day: last.day - back as u8 }
    }
}

/// A day of the year without the year, e.g. a bypass season boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonthDay {
    pub month: u8,
    pub day: u8,
}

impl MonthDay {
    /// Accepts 29 February.
    pub const fn new(month: u8, day: u8) -> Option<Self> {
        if day == 0 || day > days_in_month(2000, month) {
            return None;
        }
        Some(Self { month, day })
    }
}

/// Whether `date` lies in the inclusive window `start..=end`, which may wrap over New Year
/// (e.g. 1 October … 1 May).
pub fn in_season(date: Date, start: MonthDay, end: MonthDay) -> bool {
    let md = date.month_day();
    if start <= end { start <= md && md <= end } else { md >= start || md <= end }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDateTime {
    pub date: Date,
    /// Minutes since local midnight, 0–1439.
    pub minute: u16,
    pub second: u8,
    /// Daylight-saving time is in effect.
    pub dst: bool,
}

impl LocalDateTime {
    pub const fn weekday(&self) -> usize {
        self.date.weekday()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeZone {
    /// Standard-time offset from UTC in minutes (CET = 60).
    pub utc_offset_min: i16,
    /// Apply the EU daylight-saving rule (+60 min).
    pub eu_dst: bool,
}

impl TimeZone {
    /// Central European Time with summer time.
    pub const CET: Self = Self { utc_offset_min: 60, eu_dst: true };
    pub const UTC: Self = Self { utc_offset_min: 0, eu_dst: false };

    fn dst_active(&self, unix_s: u64) -> bool {
        if !self.eu_dst {
            return false;
        }
        let year = Date::from_days((unix_s / 86_400) as u32).year;
        let switch = |month| u64::from(Date::last_sunday(year, month).days()) * 86_400 + 3600;
        (switch(3)..switch(10)).contains(&unix_s)
    }

    /// Converts seconds since 1970-01-01 UTC to local time. Times before 1970 + offset clamp
    /// to the epoch.
    pub fn to_local(&self, unix_s: u64) -> LocalDateTime {
        let dst = self.dst_active(unix_s);
        let offset = i64::from(self.utc_offset_min) * 60 + if dst { 3600 } else { 0 };
        let local = (unix_s as i64 + offset).max(0) as u64;
        let secs_of_day = (local % 86_400) as u32;
        LocalDateTime {
            date: Date::from_days((local / 86_400) as u32),
            minute: (secs_of_day / 60) as u16,
            second: (secs_of_day % 60) as u8,
            dst,
        }
    }
}

/// Earliest plausible clock value (2024-01-01); anything earlier means "not set".
pub const MIN_VALID_UNIX_S: u64 = 1_704_067_200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_round_trip() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().days(), 0);
        assert_eq!(Date::new(2026, 10, 16).unwrap().days(), 20_742);
        for days in (0..47_000).step_by(37) {
            let d = Date::from_days(days);
            assert_eq!(d.days(), days);
            assert_eq!(Date::new(d.year, d.month, d.day), Some(d));
        }
    }

    #[test]
    fn validation_and_weekday() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2100, 1, 1).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert_eq!(Date::new(2026, 10, 16).unwrap().weekday(), 4, "Friday");
        assert_eq!(Date::new(2026, 10, 19).unwrap().weekday(), 0, "Monday");
        assert_eq!(Date::last_sunday(2026, 3), Date::new(2026, 3, 29).unwrap());
        assert_eq!(Date::last_sunday(2026, 10), Date::new(2026, 10, 25).unwrap());
    }

    #[test]
    fn eu_summer_time() {
        let tz = TimeZone::CET;
        // 2026-03-29 00:59:59 UTC is still winter time, 01:00 UTC switches.
        let before = Date::new(2026, 3, 29).unwrap().days() as u64 * 86_400 + 3599;
        let l = tz.to_local(before);
        assert_eq!((l.minute, l.dst), (119, false), "01:59 CET");
        let l = tz.to_local(before + 1);
        assert_eq!((l.minute, l.dst), (180, true), "03:00 CEST");
        // Capture time of the live unit: 2026-10-16 21:41 UTC = 23:41 CEST.
        let t = Date::new(2026, 10, 16).unwrap().days() as u64 * 86_400 + 21 * 3600 + 41 * 60;
        let l = tz.to_local(t);
        assert_eq!((l.date, l.minute, l.weekday()), (Date::new(2026, 10, 16).unwrap(), 23 * 60 + 41, 4));
        // Back to winter time on 2026-10-25 01:00 UTC.
        let back = Date::new(2026, 10, 25).unwrap().days() as u64 * 86_400 + 3600;
        assert!(tz.to_local(back - 1).dst && !tz.to_local(back).dst);
        assert!(!TimeZone::UTC.to_local(t).dst);
    }

    #[test]
    fn local_date_rolls_over_midnight() {
        let t = Date::new(2026, 12, 31).unwrap().days() as u64 * 86_400 + 23 * 3600 + 30 * 60;
        let l = TimeZone::CET.to_local(t);
        assert_eq!((l.date, l.minute), (Date::new(2027, 1, 1).unwrap(), 30));
    }

    #[test]
    fn seasons() {
        let d = |m, day| Date::new(2026, m, day).unwrap();
        let (mar, oct2) = (MonthDay::new(3, 1).unwrap(), MonthDay::new(10, 2).unwrap());
        assert!(in_season(d(3, 1), mar, oct2));
        assert!(in_season(d(10, 2), mar, oct2));
        assert!(!in_season(d(10, 3), mar, oct2));
        assert!(!in_season(d(2, 28), mar, oct2));
        // Wrapping window, e.g. the stock heating season 1 Oct … 1 May.
        let (oct, may) = (MonthDay::new(10, 1).unwrap(), MonthDay::new(5, 1).unwrap());
        assert!(in_season(d(12, 24), oct, may) && in_season(d(1, 15), oct, may));
        assert!(!in_season(d(7, 1), oct, may));
        assert!(MonthDay::new(2, 29).is_some() && MonthDay::new(4, 31).is_none());
    }
}
