//! Fan output scaling, as the stock firmware behaves.
//!
//! The EC fans take a 0–10 V control voltage, produced from TIM1 PWM through a filter.
//! The stock firmware works in volts and converts to a 0–100 % duty cycle.

/// Below this voltage the stock firmware switches the fan off entirely.
pub const CUTOFF_VOLTS: f64 = 1.65;
pub const MAX_VOLTS: f32 = 10.0;

/// Converts a control voltage to a PWM duty cycle in percent.
///
/// Matches the stock firmware's results exactly, including comparing in double precision: an `f32`
/// value of 1.65 is slightly below the `f64` constant and therefore switches the fan off.
pub fn volts_to_duty(volts: f32) -> u8 {
    // `!(x >= y)` also maps NaN to "off".
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // deliberate: NaN must take the "off" branch
    if !(f64::from(volts) >= CUTOFF_VOLTS) {
        return 0;
    }
    let duty = (f64::from(volts) * 10.0 + 0.5) as u32;
    duty.min(100) as u8
}

pub fn duty_to_volts(duty: u8) -> f32 {
    f32::from(duty.min(100)) / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_and_scaling() {
        assert_eq!(volts_to_duty(0.0), 0);
        assert_eq!(volts_to_duty(1.64), 0);
        assert_eq!(volts_to_duty(1.65), 0, "f32 1.65 < f64 1.65, as in the stock firmware");
        assert_eq!(volts_to_duty(1.66), 17);
        assert_eq!(volts_to_duty(5.0), 50);
        assert_eq!(volts_to_duty(12.0), 100);
        assert_eq!(volts_to_duty(f32::NAN), 0);
    }

    #[test]
    fn round_trip() {
        assert_eq!(volts_to_duty(duty_to_volts(73)), 73);
    }
}
