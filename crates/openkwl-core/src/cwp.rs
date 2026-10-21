//! Parser for the 248-byte heat-exchanger parameter files (`*.HWP` defaults, `*.CWP` active).
//!
//! The stock firmware uses the file as it is; the offsets below hold the frost parameters.

use crate::frost::FrostParams;

pub const LEN: usize = 248;
pub const MAGIC: [u8; 4] = [0xcc, 0x00, 0x01, 0x00];
const NAME: core::ops::Range<usize> = 4..0x18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    WrongLength(usize),
    BadMagic,
}

/// Which of the three parameter blocks is active (a stock configuration value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamSet {
    /// Config value 1 or 2.
    Base,
    /// Config value 3.
    Set3,
    /// Config value 4.
    Set4,
}

impl ParamSet {
    fn block(self) -> usize {
        match self {
            ParamSet::Base => 0x26,
            ParamSet::Set3 => 0x56,
            ParamSet::Set4 => 0x86,
        }
    }
}

fn check(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() != LEN {
        return Err(Error::WrongLength(bytes.len()));
    }
    if bytes[..4] != MAGIC {
        return Err(Error::BadMagic);
    }
    Ok(())
}

/// Heat-exchanger name, e.g. `ENTALPIE`.
pub fn name(bytes: &[u8]) -> Result<&str, Error> {
    check(bytes)?;
    let raw = &bytes[NAME];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    core::str::from_utf8(&raw[..end]).map_err(|_| Error::BadMagic)
}

/// Extracts the frost parameters. `alternate` mirrors the stock setting
/// that switches to the second threshold set (+0x18/+0x20/+0x28/+0x29/+0x34).
pub fn frost_params(bytes: &[u8], set: ParamSet, alternate: bool) -> Result<FrostParams, Error> {
    check(bytes)?;
    let b = set.block();
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let f32_at = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);

    let (arm, exit, entry, hold, boost) =
        if alternate { (0x18, 0x20, b + 2, b + 3, b + 14) } else { (0x1c, 0x21, b, b + 1, b + 10) };
    Ok(FrostParams {
        arm_below_c: f32_at(arm),
        entry_pct: f32::from(bytes[entry]),
        hold_pct: f32::from(bytes[hold]),
        exit_pct: f32::from(bytes[exit]),
        boost_delay_s: u32::from(u16_at(b + 4)) * 60,
        defrost_s: u32::from(u16_at(b + 6)) * 60,
        test_run_s: u32::from(u16_at(b + 8)),
        exhaust_boost_v: f32_at(boost),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One parameter block as written into a synthetic file: the normal values, plus the
    /// alternate entry/hold thresholds and exhaust boost.
    struct Block {
        p: FrostParams,
        alt_entry: u8,
        alt_hold: u8,
        alt_boost_v: f32,
    }

    /// Builds a 248-byte file in the stock layout (the offsets `frost_params` reads), with our
    /// own values: no Helios file is needed. `alt` is the alternate arm temperature and exit.
    fn synthetic(blocks: [&Block; 3], alt: (f32, u8)) -> [u8; LEN] {
        let mut f = [0u8; LEN];
        f[..4].copy_from_slice(&MAGIC);
        f[4..12].copy_from_slice(b"ENTALPIE");
        let base = &blocks[0].p;
        f[0x18..0x1c].copy_from_slice(&alt.0.to_le_bytes());
        f[0x1c..0x20].copy_from_slice(&base.arm_below_c.to_le_bytes());
        f[0x20] = alt.1;
        f[0x21] = base.exit_pct as u8;
        for (set, k) in [ParamSet::Base, ParamSet::Set3, ParamSet::Set4].into_iter().zip(blocks) {
            let b = set.block();
            f[b] = k.p.entry_pct as u8;
            f[b + 1] = k.p.hold_pct as u8;
            f[b + 2] = k.alt_entry;
            f[b + 3] = k.alt_hold;
            f[b + 4..b + 6].copy_from_slice(&((k.p.boost_delay_s / 60) as u16).to_le_bytes());
            f[b + 6..b + 8].copy_from_slice(&((k.p.defrost_s / 60) as u16).to_le_bytes());
            f[b + 8..b + 10].copy_from_slice(&(k.p.test_run_s as u16).to_le_bytes());
            f[b + 10..b + 14].copy_from_slice(&k.p.exhaust_boost_v.to_le_bytes());
            f[b + 14..b + 18].copy_from_slice(&k.alt_boost_v.to_le_bytes());
        }
        f
    }

    const STOCK: Block = Block { p: FrostParams::STOCK, alt_entry: 58, alt_hold: 60, alt_boost_v: 1.2 };

    #[test]
    fn reads_the_stock_values() {
        let f = synthetic([&STOCK, &STOCK, &STOCK], (-0.1, 60));
        assert_eq!(name(&f), Ok("ENTALPIE"));
        for set in [ParamSet::Base, ParamSet::Set3, ParamSet::Set4] {
            assert_eq!(frost_params(&f, set, false), Ok(FrostParams::STOCK));
        }
    }

    #[test]
    fn picks_the_selected_block() {
        let block = |n: u8| Block {
            p: FrostParams {
                entry_pct: f32::from(50 + n),
                hold_pct: f32::from(60 + n),
                boost_delay_s: u32::from(n) * 600,
                defrost_s: u32::from(n) * 3600,
                test_run_s: u32::from(n) * 100,
                exhaust_boost_v: f32::from(n) / 2.0,
                ..FrostParams::STOCK
            },
            alt_entry: 40 + n,
            alt_hold: 45 + n,
            alt_boost_v: f32::from(n),
        };
        let (one, two, three) = (block(1), block(2), block(3));
        let f = synthetic([&one, &two, &three], (-0.1, 60));
        for (set, k) in [(ParamSet::Base, &one), (ParamSet::Set3, &two), (ParamSet::Set4, &three)] {
            assert_eq!(frost_params(&f, set, false), Ok(k.p));
        }
    }

    #[test]
    fn alternate_threshold() {
        let f = synthetic([&STOCK, &STOCK, &STOCK], (-0.1, 55));
        let p = frost_params(&f, ParamSet::Base, true).unwrap();
        assert!((p.arm_below_c - -0.1).abs() < 1e-6);
        assert_eq!((p.entry_pct, p.hold_pct, p.exit_pct, p.exhaust_boost_v), (58.0, 60.0, 55.0, 1.2));
    }

    #[test]
    fn rejects_bad_input() {
        let mut f = synthetic([&STOCK, &STOCK, &STOCK], (-0.1, 60));
        assert_eq!(frost_params(&f[..100], ParamSet::Base, false), Err(Error::WrongLength(100)));
        f[0] = 0;
        assert_eq!(name(&f), Err(Error::BadMagic));
    }

    /// Checks a real parameter file from a unit's SD card, which is not part of the repository:
    /// `OPENKWL_CWP=/Volumes/<card>/04232001.CWP cargo test -p openkwl-core -- --ignored`.
    #[test]
    #[ignore = "needs a real parameter file: OPENKWL_CWP=<file>"]
    fn reads_a_real_file() {
        extern crate std;
        let path = std::env::var("OPENKWL_CWP").expect("set OPENKWL_CWP to a .CWP or .HWP file");
        let f = std::fs::read(&path).expect("readable");
        assert_eq!(name(&f), Ok("ENTALPIE"));
        // The KWL EC 300 W R ships with the stock values in every block.
        for set in [ParamSet::Base, ParamSet::Set3, ParamSet::Set4] {
            assert_eq!(frost_params(&f, set, false), Ok(FrostParams::STOCK), "{set:?}");
        }
    }
}
