//! `GET`/`PUT /config/schedule` (settings) and `/config/schedule/{1-5}` (one profile).
//!
//! The stock unit has 5 profiles ("Standard 1–3", "User defined 1–2") of 7 days with up to 10 slots each. A slot starts at
//! `from` and lasts until the next slot; before the first slot of a day the previous day's
//! last slot still applies ([`openkwl_core::schedule`]).

use openkwl_core::schedule::{self as core_schedule, MAX_SLOTS, PROFILES};
use serde::{Deserialize, Serialize};

use crate::{FieldError, Validate, check, control::MAX_LEVEL, time::TimeOfDay};

/// Schedule settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// Profile followed in auto mode, 1–5 (stock Standard 1–3, User defined 1–2).
    pub active_profile: u8,
    /// Return to auto mode every day at 00:00 (stock `v02116`).
    pub auto_at_midnight: bool,
    /// Return to auto mode this many minutes after the last manual change (10–1440);
    /// `null` = stay in manual. Not in the stock firmware.
    pub auto_return_min: Option<u16>,
    /// Level used in auto mode while the clock is not set, 0–4.
    pub fallback_level: u8,
}

/// Upper bound of the encoded [`ScheduleConfig`].
pub const SCHEDULE_CONFIG_JSON_MAX: usize = 112;

impl Validate for ScheduleConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check((1..=PROFILES as u8).contains(&self.active_profile), "active_profile", "out of range")?;
        check(self.fallback_level <= MAX_LEVEL, "fallback_level", "out of range")?;
        check(self.auto_return_min.is_none_or(|m| (10..=1440).contains(&m)), "auto_return_min", "out of range")
    }
}

impl ScheduleConfig {
    /// Value for [`openkwl_core::mode::ModeState::set_auto_return`].
    pub fn auto_return_s(&self) -> Option<u32> {
        self.auto_return_min.map(|m| u32::from(m) * 60)
    }
}

/// One switching point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// Start, `"HH:MM"`.
    pub from: TimeOfDay,
    /// Level from then on, 0–4.
    pub level: u8,
}

impl From<Slot> for core_schedule::Slot {
    fn from(s: Slot) -> Self {
        Self { start_min: s.from.0, level: s.level }
    }
}

/// Slots of one day, sorted by `from`.
pub type Day = heapless::Vec<Slot, MAX_SLOTS>;

/// One profile: Monday … Sunday.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleProfile {
    /// Exactly 7 days, Monday first.
    pub days: [Day; 7],
}

/// Upper bound of an encoded [`ScheduleProfile`] (70 slots).
pub const PROFILE_JSON_MAX: usize = 1920;

impl ScheduleProfile {
    /// Core slots of one day (0 = Monday).
    pub fn core_day(&self, weekday: usize) -> heapless::Vec<core_schedule::Slot, MAX_SLOTS> {
        self.days[weekday % 7].iter().map(|&s| s.into()).collect()
    }
}

impl Validate for ScheduleProfile {
    fn validate(&self) -> Result<(), FieldError> {
        for day in 0..7 {
            check(core_schedule::is_well_formed(&self.core_day(day), MAX_LEVEL), "days", "slots must be sorted, unique, level 0-4")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{parse, round_trip};

    fn slot(h: u16, m: u16, level: u8) -> Slot {
        Slot { from: TimeOfDay::new(h, m).unwrap(), level }
    }

    #[test]
    fn config() {
        let c = ScheduleConfig { active_profile: 1, auto_at_midnight: true, auto_return_min: Some(120), fallback_level: 2 };
        assert!(c.validate().is_ok());
        assert_eq!(c.auto_return_s(), Some(7200));
        round_trip::<_, SCHEDULE_CONFIG_JSON_MAX>(&c);
        round_trip::<_, SCHEDULE_CONFIG_JSON_MAX>(&ScheduleConfig {
            active_profile: 255,
            fallback_level: 255,
            auto_return_min: Some(u16::MAX),
            ..c
        });
        assert!(ScheduleConfig { auto_return_min: None, ..c }.validate().is_ok());
        assert_eq!(ScheduleConfig { auto_return_min: Some(5), ..c }.validate().unwrap_err().field, "auto_return_min");
        assert_eq!(ScheduleConfig { auto_return_min: Some(1441), ..c }.validate().unwrap_err().field, "auto_return_min");
        assert!(ScheduleConfig { active_profile: 5, ..c }.validate().is_ok());
        assert_eq!(ScheduleConfig { active_profile: 6, ..c }.validate().unwrap_err().field, "active_profile");
        assert_eq!(ScheduleConfig { active_profile: 0, ..c }.validate().unwrap_err().field, "active_profile");
    }

    #[test]
    fn profile_round_trip_and_core_lookup() {
        let weekday: Day = heapless::Vec::from_slice(&[slot(0, 0, 1), slot(6, 30, 3), slot(8, 0, 1), slot(22, 30, 1)]).unwrap();
        let mut days: [Day; 7] = Default::default();
        days[0] = weekday;
        let p = ScheduleProfile { days };
        assert!(p.validate().is_ok());
        let json = round_trip::<_, PROFILE_JSON_MAX>(&p);
        assert!(json.starts_with(r#"{"days":[[{"from":"00:00","level":1},{"from":"06:30","level":3}"#), "{json}");
        assert_eq!(core_schedule::level_at(|w| p.core_day(w), 0, 7 * 60), Some(3));
    }

    #[test]
    fn worst_case_fits() {
        let full: Day = (0..MAX_SLOTS as u16).map(|i| slot(10 + i, 59, 4)).collect();
        let p = ScheduleProfile { days: core::array::from_fn(|_| full.clone()) };
        assert!(p.validate().is_ok());
        round_trip::<_, PROFILE_JSON_MAX>(&p);
        let body: heapless::String<{ crate::REQUEST_BODY_MAX }> = serde_json_core::to_string(&p).expect("fits request limit");
        assert!(body.len() <= crate::REQUEST_BODY_MAX);
    }

    #[test]
    fn rejects_bad_profiles() {
        let mut days: [Day; 7] = Default::default();
        days[3] = heapless::Vec::from_slice(&[slot(8, 0, 1), slot(7, 0, 2)]).unwrap();
        assert_eq!(ScheduleProfile { days }.validate().unwrap_err().field, "days");
        assert!(parse::<ScheduleProfile>(r#"{"days":[[],[],[],[],[],[]]}"#).is_err(), "six days");
        assert!(parse::<ScheduleProfile>(r#"{"days":[[{"from":"25:00","level":1}],[],[],[],[],[],[]]}"#).is_err());
    }
}
