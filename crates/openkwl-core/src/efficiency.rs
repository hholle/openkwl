//! Heat-recovery efficiency, as computed by the stock frost-protection code.

/// η = (T_supply − T_outdoor) / (T_extract − T_outdoor) × 100 %.
///
/// Returns `None` when extract and outdoor temperatures are equal; the stock firmware then
/// keeps its previous value.
pub fn efficiency_pct(t_outdoor: f32, t_supply: f32, t_extract: f32) -> Option<f32> {
    let span = t_extract - t_outdoor;
    if span == 0.0 {
        return None;
    }
    Some((t_supply - t_outdoor) / span * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_winter_values() {
        let eta = efficiency_pct(-5.0, 16.0, 22.0).unwrap();
        assert!((eta - 77.78).abs() < 0.01);
        assert_eq!(efficiency_pct(20.0, 20.0, 20.0), None);
    }
}
