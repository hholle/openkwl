//! `GET /control`, `PATCH /control`: operating mode, level and timed programs.

use openkwl_core::mode;
use serde::{Deserialize, Serialize};

use crate::{FieldError, Validate, check};

/// Highest ventilation level (stock "Stufe 4"). Level 0 stops both fans.
pub const MAX_LEVEL: u8 = mode::MAX_LEVEL;
/// Shortest party/quiet program (stock 5 min).
pub const MIN_PROGRAM_MIN: u16 = 5;
/// Longest party/quiet program (stock 180 min).
pub const MAX_PROGRAM_MIN: u16 = 180;

/// Operating mode. Priority: party/quiet > holiday > auto/manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Runs at `level` until changed.
    Manual,
    /// Follows the weekly program (`/config/schedule`).
    Auto,
    /// Timed boost (stock "Booster"), then back to manual/auto.
    Party,
    /// Timed reduced ventilation (stock "Whisper"), then back.
    Quiet,
    /// Holiday bursts (`/config/holiday`); read-only here, set via the holiday config.
    Holiday,
}

impl From<mode::Mode> for Mode {
    fn from(m: mode::Mode) -> Self {
        match m {
            mode::Mode::Manual => Mode::Manual,
            mode::Mode::Auto => Mode::Auto,
            mode::Mode::Party => Mode::Party,
            mode::Mode::Quiet => Mode::Quiet,
            mode::Mode::Holiday => Mode::Holiday,
        }
    }
}

/// What runs when no party/quiet program or holiday overrides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseMode {
    /// The manual level (`manual_level`).
    Manual,
    /// The weekly program.
    Auto,
}

impl From<mode::Base> for BaseMode {
    fn from(b: mode::Base) -> Self {
        match b {
            mode::Base::Manual => BaseMode::Manual,
            mode::Base::Auto => BaseMode::Auto,
        }
    }
}

/// Current control state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    /// Active mode.
    pub mode: Mode,
    /// Active level, 0–4.
    pub level: u8,
    /// Seconds left in a party/quiet program, `null` otherwise.
    pub remaining_s: Option<u32>,
    /// Seconds until manual mode returns to auto (`/config/schedule` `auto_return_min`),
    /// `null` when not in manual mode or the timer is off. 0 = when the running program ends.
    pub auto_return_s: Option<u32>,
    /// What a party/quiet program or holiday returns to.
    pub base: BaseMode,
    /// Level of manual mode, also while another mode runs, 0–4.
    pub manual_level: u8,
}

impl Control {
    /// The resolved mode plus the state it returns to.
    pub fn new(e: mode::Effective, m: &mode::ModeState) -> Self {
        Self {
            mode: e.mode.into(),
            level: e.level,
            remaining_s: e.remaining_s,
            auto_return_s: e.auto_return_s,
            base: m.base.into(),
            manual_level: m.manual_level,
        }
    }
}

/// Upper bound of the encoded [`Control`].
pub const CONTROL_JSON_MAX: usize = 128;

/// Partial update.
///
/// - `{"level": n}`: during a party/quiet program changes its level, otherwise switches to
///   manual at `n`. Leaving auto starts the auto-return timer if configured.
/// - `{"mode": "manual"}` / `{"mode": "auto"}`: ends a running program; `manual` may carry a
///   `level`, `auto` may not.
/// - `{"mode": "party" | "quiet"}`: starts a program; `level` and `duration_min` default to
///   `/config/programs`.
/// - `{"end_program": true}` alone: ends a running party/quiet program early; the base mode
///   (manual level or auto) takes over. Without a program it changes nothing.
/// - `holiday` cannot be requested here; enable it in `/config/holiday`.
/// - While a holiday runs, everything but `end_program` is refused (422, `mode` or `level`);
///   end the holiday in `/config/holiday` first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequest {
    /// New mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// New level, 0–4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Program length for party/quiet, [`MIN_PROGRAM_MIN`]–[`MAX_PROGRAM_MIN`] minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<u16>,
    /// End a running party/quiet program; not combined with the other fields.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub end_program: bool,
}

impl Validate for ControlRequest {
    fn validate(&self) -> Result<(), FieldError> {
        if self.end_program {
            let alone = self.mode.is_none() && self.level.is_none() && self.duration_min.is_none();
            return check(alone, "end_program", "not combined with other fields");
        }
        check(self.mode.is_some() || self.level.is_some(), "mode", "nothing to change")?;
        if let Some(level) = self.level {
            check(level <= MAX_LEVEL, "level", "out of range")?;
        }
        match self.mode {
            Some(Mode::Holiday) => {
                return Err(FieldError { field: "mode", reason: "set via /config/holiday" });
            }
            Some(Mode::Auto) => check(self.level.is_none(), "level", "not allowed with auto")?,
            _ => {}
        }
        match (self.mode, self.duration_min) {
            (Some(Mode::Party | Mode::Quiet), Some(d)) => {
                check((MIN_PROGRAM_MIN..=MAX_PROGRAM_MIN).contains(&d), "duration_min", "out of range")
            }
            (_, Some(_)) => Err(FieldError { field: "duration_min", reason: "only for party/quiet" }),
            (_, None) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{parse, round_trip};

    #[test]
    fn control_round_trip() {
        let c = Control {
            mode: Mode::Party,
            level: 4,
            remaining_s: Some(1800),
            auto_return_s: None,
            base: BaseMode::Auto,
            manual_level: 2,
        };
        let json = round_trip::<_, CONTROL_JSON_MAX>(&c);
        assert_eq!(json, r#"{"mode":"party","level":4,"remaining_s":1800,"auto_return_s":null,"base":"auto","manual_level":2}"#);
        let manual =
            Control { mode: Mode::Manual, remaining_s: None, auto_return_s: Some(3600), base: BaseMode::Manual, level: 2, ..c };
        assert_eq!(
            round_trip::<_, CONTROL_JSON_MAX>(&manual),
            r#"{"mode":"manual","level":2,"remaining_s":null,"auto_return_s":3600,"base":"manual","manual_level":2}"#
        );
        let worst = Control {
            mode: Mode::Holiday,
            level: 4,
            remaining_s: Some(u32::MAX),
            auto_return_s: Some(u32::MAX),
            base: BaseMode::Manual,
            manual_level: 255,
        };
        round_trip::<_, CONTROL_JSON_MAX>(&worst);
    }

    #[test]
    fn from_core_state() {
        let mut m = mode::ModeState::new(mode::Base::Auto, 2);
        m.start_program(mode::ProgramKind::Quiet, 0, 600);
        let c = Control::new(m.resolve(Some(1), None, 1), &m);
        assert_eq!(
            c,
            Control {
                mode: Mode::Quiet,
                level: 0,
                remaining_s: Some(600),
                auto_return_s: None,
                base: BaseMode::Auto,
                manual_level: 2
            }
        );
        m.set_auto_return(Some(7200));
        m.set_manual();
        let c = Control::new(m.resolve(Some(1), None, 1), &m);
        assert_eq!((c.auto_return_s, c.base), (Some(7200), BaseMode::Manual));
    }

    #[test]
    fn request_partial_and_validation() {
        let r: ControlRequest = parse(r#"{"level":3}"#).unwrap();
        assert_eq!(r, ControlRequest { level: Some(3), ..Default::default() });
        assert!(r.validate().is_ok());
        round_trip::<_, 64>(&r);

        for ok in [
            r#"{"mode":"party","level":4,"duration_min":30}"#,
            r#"{"mode":"party"}"#,
            r#"{"mode":"quiet","duration_min":180}"#,
            r#"{"mode":"auto"}"#,
            r#"{"mode":"manual","level":0}"#,
            r#"{"end_program":true}"#,
        ] {
            assert!(parse::<ControlRequest>(ok).unwrap().validate().is_ok(), "{ok}");
        }

        let bad = |json: &str| parse::<ControlRequest>(json).unwrap().validate().unwrap_err().field;
        assert_eq!(bad(r#"{}"#), "mode");
        assert_eq!(bad(r#"{"level":5}"#), "level");
        assert_eq!(bad(r#"{"mode":"quiet","duration_min":4}"#), "duration_min");
        assert_eq!(bad(r#"{"mode":"quiet","duration_min":181}"#), "duration_min");
        assert_eq!(bad(r#"{"mode":"manual","duration_min":10}"#), "duration_min");
        assert_eq!(bad(r#"{"mode":"auto","level":2}"#), "level");
        assert_eq!(bad(r#"{"mode":"holiday"}"#), "mode");
        assert_eq!(bad(r#"{"end_program":true,"level":2}"#), "end_program");
        assert_eq!(bad(r#"{"end_program":false}"#), "mode");
        assert!(parse::<ControlRequest>(r#"{"mode":"turbo"}"#).is_err());
        assert!(parse::<ControlRequest>(r#"{"level":-1}"#).is_err());
    }
}
