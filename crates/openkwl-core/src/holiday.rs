//! Holiday ("Urlaub") program: between two dates the fans run at a level in bursts.
//!
//! Stock parameters (`v00602–v00606`): start/end date, level, interval 1–24 h, activation
//! period 5–300 min. The stock timing is not known; this implementation starts a burst at the
//! beginning of every interval, counted from local midnight of the start date, and keeps the
//! fans off in between. Frost protection keeps running independently.

use crate::clock::{Date, LocalDateTime, MINUTES_PER_DAY};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holiday {
    pub start: Date,
    /// Inclusive.
    pub end: Date,
    pub level: u8,
    pub interval_min: u16,
    pub active_min: u16,
}

impl Holiday {
    /// `Some(level)` during a burst, `Some(0)` between bursts, `None` outside the dates.
    pub fn level(&self, now: &LocalDateTime) -> Option<u8> {
        if now.date < self.start || now.date > self.end {
            return None;
        }
        let since_start = (now.date.days() - self.start.days()) * u32::from(MINUTES_PER_DAY) + u32::from(now.minute);
        let in_burst = since_start % u32::from(self.interval_min.max(1)) < u32::from(self.active_min);
        Some(if in_burst { self.level } else { 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u8, h: u16, m: u16) -> LocalDateTime {
        LocalDateTime { date: Date::new(2024, 7, day).unwrap(), minute: h * 60 + m, second: 0, dst: true }
    }

    /// The live unit's last holiday: 13.–19.07.2024, every 2 h for 60 min at level 1.
    const H: Holiday = Holiday {
        start: Date { year: 2024, month: 7, day: 13 },
        end: Date { year: 2024, month: 7, day: 19 },
        level: 1,
        interval_min: 120,
        active_min: 60,
    };

    #[test]
    fn bursts_inside_the_dates() {
        assert_eq!(H.level(&at(12, 23, 59)), None);
        assert_eq!(H.level(&at(13, 0, 0)), Some(1));
        assert_eq!(H.level(&at(13, 0, 59)), Some(1));
        assert_eq!(H.level(&at(13, 1, 0)), Some(0));
        assert_eq!(H.level(&at(13, 2, 0)), Some(1));
        assert_eq!(H.level(&at(19, 23, 30)), Some(0));
        assert_eq!(H.level(&at(20, 0, 0)), None);
    }

    #[test]
    fn interval_continues_across_midnight() {
        // 5 h interval: bursts at 00:00, 05:00, …, 20:00, then 01:00 on the next day.
        let h = Holiday { interval_min: 300, ..H };
        assert_eq!(h.level(&at(13, 20, 30)), Some(1));
        assert_eq!(h.level(&at(14, 0, 30)), Some(0));
        assert_eq!(h.level(&at(14, 1, 30)), Some(1));
    }
}
