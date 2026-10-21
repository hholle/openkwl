//! Filter-change reminder (stock "Info 1 – Luftfilter wechseln", `v01032`/`v01033`).
//!
//! Counts fan running time against an interval in months. A month is 30 days: the stock
//! unit with a 3-month interval reported 71 867 remaining, which fits minutes of a
//! 129 600-minute (90-day) interval.

pub const SECONDS_PER_MONTH: u32 = 30 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterCounter {
    interval_s: u32,
    elapsed_s: u32,
}

impl FilterCounter {
    pub const fn new(interval_months: u8, elapsed_s: u32) -> Self {
        Self { interval_s: interval_months as u32 * SECONDS_PER_MONTH, elapsed_s }
    }

    pub fn set_interval_months(&mut self, months: u8) {
        self.interval_s = u32::from(months) * SECONDS_PER_MONTH;
    }

    /// Call once per second.
    pub fn on_second(&mut self, fans_running: bool) {
        if fans_running {
            self.elapsed_s = self.elapsed_s.saturating_add(1);
        }
    }

    pub const fn elapsed_s(&self) -> u32 {
        self.elapsed_s
    }

    pub const fn remaining_s(&self) -> u32 {
        self.interval_s.saturating_sub(self.elapsed_s)
    }

    /// Whole days left, rounded up so "due" coincides with 0 days.
    pub const fn remaining_days(&self) -> u32 {
        self.remaining_s().div_ceil(86_400)
    }

    /// An interval of 0 months disables the reminder.
    pub const fn due(&self) -> bool {
        self.interval_s != 0 && self.remaining_s() == 0
    }

    pub fn reset(&mut self) {
        self.elapsed_s = 0;
    }

    /// Elapsed time matching a stock "remaining minutes" value (`v01033`).
    pub const fn elapsed_from_stock_remaining_min(interval_months: u8, remaining_min: u32) -> u32 {
        (interval_months as u32 * SECONDS_PER_MONTH).saturating_sub(remaining_min.saturating_mul(60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_running_time_only() {
        let mut f = FilterCounter::new(3, 0);
        assert_eq!(f.remaining_days(), 90);
        f.on_second(false);
        assert_eq!(f.elapsed_s(), 0);
        f.on_second(true);
        assert_eq!(f.remaining_days(), 90, "rounded up");
        assert!(!f.due());
    }

    #[test]
    fn due_and_reset() {
        let mut f = FilterCounter::new(1, SECONDS_PER_MONTH - 1);
        f.on_second(true);
        assert!(f.due() && f.remaining_days() == 0);
        f.reset();
        assert!(!f.due());
        assert!(!FilterCounter::new(0, 0).due(), "disabled");
    }

    #[test]
    fn imports_stock_remaining_minutes() {
        let elapsed = FilterCounter::elapsed_from_stock_remaining_min(3, 71_867);
        let f = FilterCounter::new(3, elapsed);
        assert_eq!(f.remaining_days(), 50);
    }
}
