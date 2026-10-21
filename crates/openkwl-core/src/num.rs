//! A small decimal → `f32` parser. New code (no stock counterpart).
//!
//! `core`'s `f32::from_str` is exact for every input but brings a 10 KB table (Eisel–Lemire),
//! about 6 % of the firmware slot. The numbers this unit parses (settings, JSON from the web
//! UI, Home Assistant commands) are short decimals like `21.5` or `-3.25`; for up to 15
//! significant digits and decimal exponents within ±22 this parser takes Clinger's fast path
//! (one exact `f64` multiplication or division, then rounding to `f32`) and is exactly as
//! precise as `core`. Longer inputs are still parsed, to within a few ulp.

/// Exact powers of ten in `f64` (10^0 … 10^22).
const POW10: [f64; 23] =
    [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];

fn scale(mut v: f64, mut exp: i32) -> f64 {
    while exp > 22 {
        v *= 1e22;
        exp -= 22;
    }
    while exp < -22 {
        v /= 1e22;
        exp += 22;
    }
    if exp >= 0 { v * POW10[exp as usize] } else { v / POW10[(-exp) as usize] }
}

/// Parses `[+-]digits[.digits][(e|E)[+-]digits]` like `str::parse::<f32>` does (also `inf`,
/// `nan`, `.5`, `5.`); `None` for anything else, including empty input.
pub fn parse_f32(s: &str) -> Option<f32> {
    let b = s.as_bytes();
    let (neg, mut i) = match b.first()? {
        b'-' => (true, 1),
        b'+' => (false, 1),
        _ => (false, 0),
    };
    let rest = &s[i..];
    if rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity") {
        return Some(if neg { f32::NEG_INFINITY } else { f32::INFINITY });
    }
    if rest.eq_ignore_ascii_case("nan") {
        return Some(f32::NAN);
    }

    let mut mantissa: u64 = 0;
    let mut exp: i32 = 0;
    let mut seen_digit = false;
    let mut in_fraction = false;
    while i < b.len() {
        match b[i] {
            c @ b'0'..=b'9' => {
                seen_digit = true;
                if mantissa < 100_000_000_000_000_000 {
                    mantissa = mantissa * 10 + u64::from(c - b'0');
                    if in_fraction {
                        exp -= 1;
                    }
                } else if !in_fraction {
                    exp += 1; // digits beyond u64 precision still count in the integer part
                }
            }
            b'.' if !in_fraction => in_fraction = true,
            b'e' | b'E' => break,
            _ => return None,
        }
        i += 1;
    }
    if !seen_digit {
        return None;
    }
    if i < b.len() {
        // Exponent.
        i += 1;
        let (eneg, start) = match b.get(i)? {
            b'-' => (true, i + 1),
            b'+' => (false, i + 1),
            _ => (false, i),
        };
        let e = &s[start..];
        if e.is_empty() || !e.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let e: i32 = e.parse::<u16>().map_or(10_000, i32::from);
        exp += if eneg { -e } else { e };
    }
    let v = scale(mantissa as f64, exp.clamp(-400, 400)) as f32;
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn matches_core_on_typical_inputs() {
        for s in [
            "0",
            "-0",
            "21.5",
            "-3.14",
            "0.1",
            "0.2",
            "0.3",
            "1.65",
            "22.0",
            "100",
            "1e3",
            "1E-3",
            "2.5e+2",
            ".5",
            "5.",
            "+7",
            "35",
            "9.999",
            "-12.75",
            "0.000001",
            "123456.789",
            "3.4028235e38",
            "1e-45",
            "16777217",
        ] {
            let ours = parse_f32(s).unwrap();
            let core: f32 = s.parse().unwrap();
            assert_eq!(ours.to_bits(), core.to_bits(), "{s}: {ours} vs {core}");
        }
    }

    #[test]
    fn exact_for_short_decimals() {
        // Every value with one or two decimals in the range the settings use.
        for i in -5000..=5000 {
            for s in [alloc_fmt(i, 1), alloc_fmt(i, 2)] {
                let core: f32 = s.parse().unwrap();
                assert_eq!(parse_f32(&s).unwrap().to_bits(), core.to_bits(), "{s}");
            }
        }
    }

    fn alloc_fmt(i: i32, decimals: u32) -> std::string::String {
        let d = 10i32.pow(decimals);
        let sign = if i < 0 { "-" } else { "" };
        let a = i.abs();
        std::format!("{sign}{}.{:0w$}", a / d, a % d, w = decimals as usize)
    }

    #[test]
    fn specials_and_errors() {
        assert_eq!(parse_f32("inf"), Some(f32::INFINITY));
        assert_eq!(parse_f32("-inf"), Some(f32::NEG_INFINITY));
        assert!(parse_f32("NaN").unwrap().is_nan());
        for bad in ["", "-", ".", "e5", "1e", "1e+", "1.2.3", "12a", " 1", "1,5"] {
            assert_eq!(parse_f32(bad), None, "{bad}");
        }
    }
}
