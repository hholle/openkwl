//! Air-flow estimate from the measured fan speed. New behaviour (the stock firmware has no
//! air-flow figure for the KWL EC 200/300/500 W: Helios classes them "drehzahlkonstant", and
//! commissioning reads the flow off printed curves).
//!
//! The EC fans hold their speed, so over a fixed duct system (p = c·Q²) the fan affinity law
//! gives **Q = k·n**. `k` depends on the ducts; the default comes from the Helios catalogue
//! curves of the KWL EC 300 W (315 m³/h at 10 V and 0 Pa … 279 m³/h at 100 Pa) intersected with
//! the DIBt reference system (100 Pa at 280 m³/h, approval Z-51.3-438), with ≈ 2370 rpm at 10 V
//! extrapolated from the owner's unit (1350 rpm at 6 V): k ≈ 0.118 m³/h per rpm. Expected error
//! without calibration ±15–25 %; one anemometer or duct-pressure reading brings it to ≈ ±10 %
//! (`docs/AIRFLOW.md`).

/// Default for the KWL EC 300 W: m³/h per 1000 rpm.
pub const KWL_EC_300_M3H_PER_KRPM: u16 = 118;

/// Below this speed the tacho is unreliable and the flow is treated as unknown.
pub const MIN_RPM: u32 = 200;

/// `rpm × k`; `None` when the fan is (nearly) stopped or `k` is 0 (no estimate wanted).
pub fn estimate_m3h(rpm: u32, m3h_per_krpm: u16) -> Option<f32> {
    (rpm >= MIN_RPM && m3h_per_krpm > 0).then(|| rpm as f32 * f32::from(m3h_per_krpm) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_unit_at_level_2() {
        // 6 V, 1350 rpm: the 6 V catalogue curve meets the reference system at ≈ 155 m³/h.
        let q = estimate_m3h(1350, KWL_EC_300_M3H_PER_KRPM).unwrap();
        assert!((150.0..165.0).contains(&q), "{q}");
    }

    #[test]
    fn unknown_when_stopped_or_disabled() {
        assert_eq!(estimate_m3h(0, KWL_EC_300_M3H_PER_KRPM), None);
        assert_eq!(estimate_m3h(150, KWL_EC_300_M3H_PER_KRPM), None);
        assert_eq!(estimate_m3h(1350, 0), None);
    }
}
