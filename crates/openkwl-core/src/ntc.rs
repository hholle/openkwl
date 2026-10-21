//! NTC temperature conversion, as the stock firmware behaves.
//!
//! Each sensor is a 2252 Ω @ 25 °C NTC to ground with a 10 kΩ pull-up, read by the 12-bit
//! ADC against a 2.5 V reference. The stock firmware converts with the Steinhart–Hart
//! equation; the constants below give the same results as the stock firmware.

use libm::log;

/// Raw readings above this are treated as an open circuit (cable break).
pub const RAW_MAX_VALID: u16 = 4000;
/// Raw readings below this are treated as a short circuit.
pub const RAW_MIN_VALID: u16 = 50;

const ADC_VREF: f32 = 2.5;
const ADC_FULL_SCALE: f32 = 4096.0;
const PULLUP_OHMS: f32 = 10_000.0;

// Steinhart–Hart coefficients, the same values the stock firmware uses.
const SH_A: f64 = f64::from_bits(0x3f58_3f91_e646_f156); // 1.48e-3
const SH_B: f64 = f64::from_bits(0x3f2f_0e0d_33f0_1c88); // 2.3693e-4
const SH_C: f64 = f64::from_bits(0x3e7d_8724_7702_c0d0); // 1.1e-7
const KELVIN_OFFSET: f64 = f64::from_bits(0x4071_1266_6666_6666); // 273.15

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reading {
    Celsius(f32),
    /// Sensor missing or cable broken ("fehlt od. Kabelbruch", errors 10–13).
    Open,
    /// Sensor shorted ("Kurzschluss", errors 14–17).
    Short,
}

impl Reading {
    pub fn celsius(self) -> Option<f32> {
        match self {
            Reading::Celsius(c) => Some(c),
            _ => None,
        }
    }
}

/// Converts a (smoothed) raw ADC value to a temperature.
///
/// `supply_volts` is the pull-up supply. The stock firmware uses a measured value stored
/// next to the sensor block; on a ratiometric design it equals the 2.5 V reference.
pub fn raw_to_reading(raw: u16, supply_volts: f32) -> Reading {
    if raw > RAW_MAX_VALID {
        return Reading::Open;
    }
    if raw < RAW_MIN_VALID {
        return Reading::Short;
    }
    let volts = f32::from(raw) * (ADC_VREF / ADC_FULL_SCALE);
    let headroom = supply_volts - volts;
    if headroom <= 0.0 {
        return Reading::Open;
    }
    // The stock firmware truncates the resistance to a u16 before taking the logarithm.
    let ohms = (PULLUP_OHMS * volts / headroom).min(f32::from(u16::MAX)) as u16;
    if ohms == 0 {
        return Reading::Short;
    }
    Reading::Celsius(ohms_to_celsius(f32::from(ohms)))
}

/// The pull-up supply from its (smoothed) raw reading on PC3: the board halves it for the
/// ADC, so the stock firmware takes V × 2, rounded to 0.01 V.
pub fn supply_volts(raw: u16) -> f32 {
    let volts = f32::from(raw) * (ADC_VREF / ADC_FULL_SCALE) * 2.0;
    (volts * 100.0 + 0.5) as u32 as f32 / 100.0
}

/// Steinhart–Hart: T = 1 / (A + B·ln R + C·ln³ R) − 273.15.
pub fn ohms_to_celsius(ohms: f32) -> f32 {
    let ln_r = log(f64::from(ohms));
    (1.0 / (SH_A + SH_B * ln_r + SH_C * ln_r * ln_r * ln_r) - KELVIN_OFFSET) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn matches_stock_curve() {
        // The stock coefficients put 2252 Ω at 24.5 °C, about 0.5 K below the nominal
        // 25 °C point of a 2252 Ω NTC. Kept as-is for fidelity with the stock firmware.
        assert!(close(ohms_to_celsius(2252.0), 24.50), "{}", ohms_to_celsius(2252.0));
        assert!(close(ohms_to_celsius(2000.0), 27.22));
        assert!(close(ohms_to_celsius(5000.0), 7.28));
    }

    #[test]
    fn detects_open_and_short() {
        assert_eq!(raw_to_reading(4001, 2.5), Reading::Open);
        assert_eq!(raw_to_reading(49, 2.5), Reading::Short);
        assert!(raw_to_reading(1500, 2.5).celsius().is_some());
    }

    #[test]
    fn higher_raw_means_colder() {
        // NTC to ground: more resistance (colder) gives a higher voltage.
        let warm = raw_to_reading(1000, 2.5).celsius().unwrap();
        let cold = raw_to_reading(3000, 2.5).celsius().unwrap();
        assert!(cold < warm);
    }

    #[test]
    fn supply_is_twice_the_pin_rounded() {
        assert_eq!(supply_volts(2048), 2.5);
        assert_eq!(supply_volts(2000), 2.44);
        assert_eq!(supply_volts(0), 0.0);
    }

    #[test]
    fn a_lower_supply_reads_colder() {
        // The same pin voltage with less supply means a larger NTC resistance, i.e. colder.
        let (Reading::Celsius(at_2v5), Reading::Celsius(at_2v45)) = (raw_to_reading(900, 2.5), raw_to_reading(900, 2.45)) else {
            panic!()
        };
        assert!(at_2v45 < at_2v5);
    }
}
