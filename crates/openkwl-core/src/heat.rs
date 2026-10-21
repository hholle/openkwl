//! Heat-recovery estimate.
//!
//! P = ρ·c_p · V̇ · (T_supply − T_outdoor). The air flow is not measured; it comes from a
//! configured m³/h per level, so the result is an estimate. The supply sensor sits after the
//! fan, so fan heat is included. Negative power means the supply air was cooled (summer).

/// ρ·c_p of air near room conditions, Wh/(m³·K).
pub const AIR_WH_PER_M3_K: f32 = 0.335;

pub fn recovered_power_w(airflow_m3h: f32, t_outdoor: f32, t_supply: f32) -> f32 {
    AIR_WH_PER_M3_K * airflow_m3h * (t_supply - t_outdoor)
}

/// Accumulates recovered energy in whole joules, separately for heating and cooling, so
/// totals do not lose precision over years. Persist with [`EnergyCounter::raw`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnergyCounter {
    heating_j: u64,
    cooling_j: u64,
}

impl EnergyCounter {
    pub const fn from_raw(heating_j: u64, cooling_j: u64) -> Self {
        Self { heating_j, cooling_j }
    }

    pub const fn raw(&self) -> (u64, u64) {
        (self.heating_j, self.cooling_j)
    }

    /// Adds one second at `power_w`.
    pub fn add_second(&mut self, power_w: f32) {
        if !power_w.is_finite() {
            return;
        }
        let j = libm::roundf(power_w.abs()) as u64;
        if power_w > 0.0 {
            self.heating_j = self.heating_j.saturating_add(j);
        } else {
            self.cooling_j = self.cooling_j.saturating_add(j);
        }
    }

    pub fn heating_kwh(&self) -> f32 {
        (self.heating_j as f64 / 3.6e6) as f32
    }

    pub fn cooling_kwh(&self) -> f32 {
        (self.cooling_j as f64 / 3.6e6) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_unit_example() {
        // Live capture: 12.8 °C outdoor, 19.8 °C supply; assume 150 m³/h → ≈ 352 W.
        let p = recovered_power_w(150.0, 12.8, 19.8);
        assert!((p - 351.75).abs() < 0.1, "{p}");
        assert!(recovered_power_w(150.0, 30.0, 25.0) < 0.0, "summer cooling");
    }

    #[test]
    fn energy_accumulates() {
        let mut e = EnergyCounter::default();
        for _ in 0..3600 {
            e.add_second(1000.0);
        }
        assert!((e.heating_kwh() - 1.0).abs() < 1e-6);
        e.add_second(-500.0);
        e.add_second(f32::NAN);
        assert_eq!(e.raw(), (3_600_000, 500));
        assert_eq!(EnergyCounter::from_raw(1, 2).raw(), (1, 2));
    }
}
