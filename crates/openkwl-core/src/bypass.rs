//! Summer bypass decision; reimplements the stock firmware's behaviour (same thresholds and
//! states), including which boundary cases count as "at or above" and "at or below".

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BypassParams {
    /// Room/extract target temperature (web parameter `v01035`).
    pub target_c: f32,
    /// Minimum outdoor temperature (web parameter `v01036`).
    pub min_outdoor_c: f32,
    /// Stock bypass "mode 2": close again when outdoor air gets more than
    /// 2 K warmer than extract air. With this off, the stock firmware opens even when outdoor
    /// air is warmer than extract air.
    pub close_if_outdoor_warmer: bool,
}

/// Hysteresis below `target_c` before an open bypass closes.
pub const TARGET_HYSTERESIS_K: f32 = 2.0;
/// Hysteresis below `min_outdoor_c` before an open bypass closes.
pub const MIN_OUTDOOR_HYSTERESIS_K: f32 = 1.0;
/// Margin by which outdoor air may exceed extract air in mode 2 before closing.
pub const OUTDOOR_WARMER_MARGIN_K: f32 = 2.0;

/// Returns whether the bypass should be open.
///
/// `t_extract` is the room sensor if fitted, otherwise T3 (extract air); `t_outdoor` is an
/// external outdoor sensor if fitted, otherwise T1.
pub fn decide(currently_open: bool, p: &BypassParams, t_extract: f32, t_outdoor: f32) -> bool {
    // Is the house warm enough to want cooling at all?
    if currently_open {
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // deliberate: NaN must take the "close" branch
        if !(p.target_c - TARGET_HYSTERESIS_K < t_extract) {
            return false;
        }
    } else if t_extract <= p.target_c {
        return false;
    }

    // Too cold outside?
    if p.min_outdoor_c >= t_outdoor {
        return currently_open && p.min_outdoor_c - MIN_OUTDOOR_HYSTERESIS_K <= t_outdoor;
    }

    // Outdoor air warmer than inside?
    if t_outdoor >= t_extract {
        if !p.close_if_outdoor_warmer {
            return true;
        }
        return currently_open && t_extract + OUTDOOR_WARMER_MARGIN_K >= t_outdoor;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: BypassParams = BypassParams { target_c: 23.0, min_outdoor_c: 15.0, close_if_outdoor_warmer: true };

    #[test]
    fn opens_on_cool_summer_evening() {
        assert!(decide(false, &P, 25.0, 18.0));
    }

    #[test]
    fn stays_closed_when_house_not_warm() {
        assert!(!decide(false, &P, 23.0, 18.0));
    }

    #[test]
    fn stays_closed_when_outdoor_too_cold() {
        assert!(!decide(false, &P, 25.0, 14.0));
    }

    #[test]
    fn target_hysteresis() {
        assert!(decide(true, &P, 21.5, 18.0), "still above 23 − 2");
        assert!(!decide(true, &P, 21.0, 18.0), "closes at 23 − 2");
    }

    #[test]
    fn min_outdoor_hysteresis() {
        assert!(decide(true, &P, 25.0, 14.5), "within 1 K below the minimum");
        assert!(!decide(true, &P, 25.0, 13.9));
    }

    #[test]
    fn outdoor_warmer_than_inside() {
        assert!(!decide(false, &P, 25.0, 26.0), "never opens into warmer air");
        assert!(decide(true, &P, 25.0, 26.5), "tolerates up to 2 K while open");
        assert!(!decide(true, &P, 25.0, 27.5));
        let lax = BypassParams { close_if_outdoor_warmer: false, ..P };
        assert!(decide(false, &lax, 25.0, 26.0), "stock non-mode-2 behaviour");
    }
}
