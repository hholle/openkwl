//! Fan speed from tacho pulses: pulse period measured by a timer, speed updated every second,
//! as the stock firmware behaves.

use crate::average::MovingAverage;

/// Stock conversion: rpm = 3 156 000 / timer ticks, with TIM4/TIM5 at prescaler 950.
pub const STOCK_RPM_NUMERATOR: u32 = 3_156_000;
/// Readings above this are discarded as glitches.
pub const MAX_RPM: u32 = 10_000;
/// A fan that produces fewer pulses than this within one second is reported as stopped.
pub const MIN_PULSES_PER_SECOND: u8 = 3;

/// The stock formula, valid only with the stock timer configuration.
pub fn rpm_from_stock_ticks(ticks: u32) -> u32 {
    clamp_rpm(STOCK_RPM_NUMERATOR / ticks.max(1))
}

/// Timer-independent formula for a pulse period measured in microseconds.
///
/// `pulses_per_rev` is not known yet; calibrate it on the bench against the stock reading.
pub fn rpm_from_period_us(period_us: u32, pulses_per_rev: u32) -> u32 {
    let denom = u64::from(period_us.max(1)) * u64::from(pulses_per_rev.max(1));
    clamp_rpm((60_000_000u64 / denom).min(u64::from(u32::MAX)) as u32)
}

fn clamp_rpm(rpm: u32) -> u32 {
    if rpm > MAX_RPM { 0 } else { rpm }
}

#[derive(Debug, Clone, Default)]
pub struct Tacho {
    avg: MovingAverage<8>,
    rpm: u32,
    pulses_this_second: u8,
}

impl Tacho {
    pub const fn new() -> Self {
        Self { avg: MovingAverage::new(), rpm: 0, pulses_this_second: 0 }
    }

    /// Records one tacho pulse with its already-converted instantaneous speed.
    pub fn on_pulse(&mut self, instant_rpm: u32) {
        self.rpm = self.avg.push(instant_rpm);
        self.pulses_this_second = self.pulses_this_second.saturating_add(1);
    }

    /// Call once per second. Like the stock firmware, the counter restarts at 1.
    pub fn on_second(&mut self) {
        if self.pulses_this_second < MIN_PULSES_PER_SECOND {
            self.rpm = 0;
        }
        self.pulses_this_second = 1;
    }

    pub fn rpm(&self) -> u32 {
        self.rpm
    }
}

/// Flags a fan that is commanded to run but does not reach `min_rpm` for `limit_s` seconds.
///
/// The stock firmware has the same mechanism ("Fehler 01/02 Drehzahlfehler"); its exact
/// thresholds are not known, so they are parameters here.
#[derive(Debug, Clone, Copy)]
pub struct StallDetector {
    pub min_rpm: u32,
    pub limit_s: u32,
    low_for_s: u32,
}

impl StallDetector {
    pub const fn new(min_rpm: u32, limit_s: u32) -> Self {
        Self { min_rpm, limit_s, low_for_s: 0 }
    }

    /// Call once per second; returns `true` while the fan is considered stalled.
    pub fn on_second(&mut self, commanded_duty: u8, rpm: u32) -> bool {
        if commanded_duty > 0 && rpm < self.min_rpm {
            self.low_for_s = self.low_for_s.saturating_add(1);
        } else {
            self.low_for_s = 0;
        }
        self.stalled()
    }

    pub fn stalled(&self) -> bool {
        self.low_for_s >= self.limit_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_formula() {
        assert_eq!(rpm_from_stock_ticks(3156), 1000);
        assert_eq!(rpm_from_stock_ticks(0), 0, "division guard then glitch filter");
        assert_eq!(rpm_from_stock_ticks(100), 0, "31 560 rpm is discarded");
    }

    #[test]
    fn period_formula() {
        assert_eq!(rpm_from_period_us(60_000, 1), 1000);
        assert_eq!(rpm_from_period_us(30_000, 2), 1000);
    }

    #[test]
    fn stops_without_pulses() {
        let mut t = Tacho::new();
        for _ in 0..8 {
            t.on_pulse(1200);
        }
        t.on_second();
        assert_eq!(t.rpm(), 1200);
        t.on_pulse(1200); // only 2 pulses counted this second (counter restarts at 1)
        t.on_second();
        assert_eq!(t.rpm(), 0);
    }

    #[test]
    fn stall_detection() {
        let mut s = StallDetector::new(300, 3);
        assert!(!s.on_second(50, 0));
        assert!(!s.on_second(50, 0));
        assert!(s.on_second(50, 0));
        assert!(!s.on_second(50, 900), "recovers once spinning");
        assert!(!s.on_second(0, 0), "a fan commanded off is never stalled");
    }
}
