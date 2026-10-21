//! When to write to the SD card (write budget, `docs/PLAN.md` §3.1).

/// Settings: write once changes have been quiet for `delay_s`, so dragging a slider gives
/// one write, not fifty. A change that keeps arriving is still written after `max_delay_s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debounce {
    pub delay_s: u64,
    pub max_delay_s: u64,
    first_change_s: Option<u64>,
    last_change_s: u64,
}

impl Debounce {
    pub const fn new(delay_s: u64, max_delay_s: u64) -> Self {
        Self { delay_s, max_delay_s, first_change_s: None, last_change_s: 0 }
    }

    /// Settings: 10 s quiet, at most 60 s.
    pub const SETTINGS: Self = Self::new(10, 60);
    /// Mode and manual level (a 64-byte state record): 60 s quiet, at most 10 min, so a burst
    /// of changes from Home Assistant costs one write.
    pub const MODE: Self = Self::new(60, 600);

    pub fn changed(&mut self, now_s: u64) {
        self.first_change_s.get_or_insert(now_s);
        self.last_change_s = now_s;
    }

    pub fn dirty(&self) -> bool {
        self.first_change_s.is_some()
    }

    pub fn due(&self, now_s: u64) -> bool {
        match self.first_change_s {
            None => false,
            Some(first) => now_s >= self.last_change_s + self.delay_s || now_s >= first + self.max_delay_s,
        }
    }

    /// Call after a successful write.
    pub fn written(&mut self) {
        self.first_change_s = None;
    }
}

/// Runtime counters: every `interval_s` (default 6 h), plus whenever the firmware calls
/// [`Periodic::force`] (clean reboot, power-fail warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periodic {
    pub interval_s: u64,
    last_s: u64,
    forced: bool,
}

impl Periodic {
    pub const STATE: Self = Self { interval_s: 6 * 3600, last_s: 0, forced: false };

    pub fn due(&self, now_s: u64) -> bool {
        self.forced || now_s >= self.last_s + self.interval_s
    }

    pub fn force(&mut self) {
        self.forced = true;
    }

    pub fn written(&mut self, now_s: u64) {
        self.last_s = now_s;
        self.forced = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_waits_for_quiet() {
        let mut d = Debounce::SETTINGS;
        assert!(!d.due(100));
        d.changed(100);
        d.changed(105);
        assert!(!d.due(114));
        assert!(d.due(115));
        d.written();
        assert!(!d.dirty() && !d.due(1000));
    }

    #[test]
    fn debounce_caps_continuous_changes() {
        let mut d = Debounce::SETTINGS;
        for t in (0..=60).step_by(5) {
            d.changed(t);
        }
        assert!(d.due(60), "60 s after the first change");
    }

    #[test]
    fn periodic() {
        let mut p = Periodic::STATE;
        assert!(!p.due(6 * 3600 - 1));
        assert!(p.due(6 * 3600));
        p.written(6 * 3600);
        p.force();
        assert!(p.due(6 * 3600 + 1));
        p.written(6 * 3600 + 1);
        assert!(!p.due(6 * 3600 + 2));
    }
}
