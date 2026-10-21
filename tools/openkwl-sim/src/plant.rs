//! A simple thermal model of the unit and the house, good enough to exercise the controller.
//!
//! - Outdoor air follows a daily sine (coldest at 05:00) around `outdoor_mean_c`, or stays fixed.
//! - The heat exchanger recovers `η` of the temperature difference; η falls slightly with
//!   the fan level. With `icing`, η decays below −3 °C outdoors while cold air flows and
//!   recovers when the supply fan is stopped (defrost), as frost protection expects.
//! - Fans spin up and down with a first-order lag; rpm ≈ 225 per volt.
//! - The house drifts towards its own equilibrium; an open bypass cools it with outdoor air.

use openkwl_app::{Inputs, Outputs};
use openkwl_core::ntc::Reading;

#[derive(Debug, Clone)]
pub struct Plant {
    pub outdoor_mean_c: f32,
    pub outdoor_fixed: bool,
    pub icing: bool,
    pub room_c: f32,
    /// Ice on the exchanger, 0 (none) … 1 (blocked).
    pub ice: f32,
    supply_rpm: f32,
    exhaust_rpm: f32,
    supply_c: f32,
    exhaust_c: f32,
    seed: u32,
}

impl Plant {
    pub fn new(outdoor_mean_c: f32, outdoor_fixed: bool, icing: bool) -> Self {
        Self {
            outdoor_mean_c,
            outdoor_fixed,
            icing,
            room_c: 22.1,
            ice: 0.0,
            supply_rpm: 0.0,
            exhaust_rpm: 0.0,
            supply_c: 19.8,
            exhaust_c: 13.7,
            seed: 0x1234_5678,
        }
    }

    fn noise(&mut self) -> f32 {
        // xorshift; ±0.05 K sensor noise.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 1000) as f32 / 10_000.0 - 0.05
    }

    pub fn outdoor_c(&self, local_hour: f32) -> f32 {
        if self.outdoor_fixed {
            return self.outdoor_mean_c;
        }
        let phase = (local_hour - 5.0) / 24.0 * core::f32::consts::TAU;
        self.outdoor_mean_c - 4.0 * phase.cos()
    }

    /// Advances one second with the controller's last outputs; returns the new readings.
    pub fn step(&mut self, out: &Outputs, local_hour: f32) -> Inputs {
        let outdoor = self.outdoor_c(local_hour);
        let lag = |rpm: &mut f32, volts: f32| *rpm += (volts * 225.0 - *rpm) * 0.25;
        lag(&mut self.supply_rpm, out.supply_volts);
        lag(&mut self.exhaust_rpm, out.exhaust_volts * 0.97);
        let supply_on = self.supply_rpm > 100.0;
        let exhaust_on = self.exhaust_rpm > 100.0;

        // Icing: grows while cold supply air flows, melts while the supply fan is stopped.
        if self.icing && outdoor < -3.0 && supply_on && !out.bypass_open {
            self.ice = (self.ice + 1.0 / 1800.0).min(1.0);
        } else if !supply_on && exhaust_on {
            self.ice = (self.ice - 1.0 / 3600.0).max(0.0);
        }
        let level_factor = (out.supply_volts / 10.0).clamp(0.0, 1.0);
        let eta = (0.82 - 0.06 * level_factor) * (1.0 - 0.6 * self.ice);

        let supply_target = if !supply_on {
            self.room_c - 1.5
        } else if out.bypass_open {
            outdoor + 0.6
        } else {
            outdoor + eta * (self.room_c - outdoor) + 0.4
        };
        let exhaust_target = if !exhaust_on {
            self.room_c - 3.0
        } else if out.bypass_open {
            self.room_c - 0.3
        } else {
            self.room_c - eta * (self.room_c - outdoor) * if supply_on { 1.0 } else { 0.15 } + 0.6
        };
        self.supply_c += (supply_target - self.supply_c) * 0.05;
        self.exhaust_c += (exhaust_target - self.exhaust_c) * 0.05;

        // House: equilibrium 21.5 °C with heating; ventilation pulls towards supply air.
        let flow = level_factor * f32::from(u8::from(supply_on));
        self.room_c += ((21.5 - self.room_c) / 20_000.0) + (self.supply_c - self.room_c) * flow / 40_000.0;

        let r = |c: f32, n: f32| Reading::Celsius(((c + n) * 10.0).round() / 10.0);
        Inputs {
            outdoor: r(outdoor, self.noise()),
            supply: r(self.supply_c, self.noise()),
            extract: r(self.room_c, self.noise()),
            exhaust: r(self.exhaust_c, self.noise()),
            supply_rpm: self.supply_rpm as u32,
            exhaust_rpm: self.exhaust_rpm as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut Plant, out: &Outputs, seconds: u32) -> Inputs {
        let mut i = p.step(out, 12.0);
        for _ in 1..seconds {
            i = p.step(out, 12.0);
        }
        i
    }

    const LEVEL2: Outputs = Outputs { supply_volts: 6.0, exhaust_volts: 6.0, supply_duty: 60, exhaust_duty: 60, bypass_open: false };

    #[test]
    fn recovers_heat_at_steady_state() {
        let mut p = Plant::new(10.0, true, false);
        let i = run(&mut p, &LEVEL2, 600);
        let (o, s, e) = (i.outdoor.celsius().unwrap(), i.supply.celsius().unwrap(), i.extract.celsius().unwrap());
        let eta = (s - o) / (e - o);
        assert!((0.7..0.85).contains(&eta), "η = {eta}");
        assert!((1300..1360).contains(&i.supply_rpm));
    }

    #[test]
    fn ices_up_and_thaws() {
        let mut p = Plant::new(-8.0, true, true);
        run(&mut p, &LEVEL2, 1800);
        assert!(p.ice > 0.9);
        let defrost = Outputs { supply_volts: 0.0, supply_duty: 0, ..LEVEL2 };
        run(&mut p, &defrost, 3600);
        assert!(p.ice < 0.1);
    }

    #[test]
    fn daily_outdoor_curve() {
        let p = Plant::new(10.0, false, false);
        assert!((p.outdoor_c(5.0) - 6.0).abs() < 0.01);
        assert!((p.outdoor_c(17.0) - 14.0).abs() < 0.01);
    }
}
