//! Request and response types for the device REST API (`/api/v1`).
//!
//! These types are the single Rust definition of the contract in `api/openapi.yaml`. The
//! firmware (de)serialises them with `serde-json-core` into fixed buffers, so every string
//! and list is a bounded `heapless` type and every `*_JSON_MAX` constant is a tested upper
//! bound for the encoded size.
//!
//! JSON conventions: `snake_case` names, temperatures in °C, voltages in V, durations in
//! seconds unless a field name says otherwise, absent sensor readings as `null`.
//!
//! Request bodies are checked twice: by serde (types, string capacity) and by the
//! [`Validate`] impls (ranges, formats). Handlers must call `validate()` before applying a
//! request.

#![no_std]
#![warn(missing_docs)]

/// Login, logout, password change.
pub mod auth;
/// Persistent settings.
pub mod config;
/// Mode and level.
pub mod control;
/// Error bodies.
pub mod error;
/// Reading request bodies without serde (smaller firmware); serde stays for writing.
pub mod json;
/// Weekly program.
pub mod schedule;
/// Live readings.
pub mod status;
/// Device information.
pub mod system;
pub mod time;

pub use auth::{LoginRequest, LoginResponse, PasswordChangeRequest, SetupRequest, SetupStatus};
pub use config::{
    ActuatorType, BypassConfig, FanConfig, FilterConfig, FrostConfig, HolidayConfig, MqttConfig, MqttConfigUpdate, NetworkConfig,
    ProgramConfig, Season, TimeConfig,
};
pub use control::{BaseMode, Control, ControlRequest, Mode};
pub use error::{ApiError, ErrorCode, FieldError};
pub use schedule::{ScheduleConfig, ScheduleProfile, Slot};
pub use status::{
    BypassStatus, Fan, Fault, FilterStatus, FrostState, FrostStatus, HeatRecovery, HolidayStatus, NextChange, ScheduleToday, Status,
    Temperatures,
};
pub use system::{
    BoardDetails, BusPower, ChipDetails, Counters, DeviceIdentity, FirmwareDetails, KnxState, MemoryDetails, ModuleDetails,
    MqttDetails, NetworkDetails, ResetReason, SetTimeRequest, StockDetails, StorageDetails, SystemDetails, SystemInfo, TimeDetails,
    TimeSource,
};

/// Largest request body the device accepts; larger bodies get 413. Fits every request type
/// at full capacity without escapes (tested); the largest is a full schedule profile.
pub const REQUEST_BODY_MAX: usize = 2048;

/// Path prefix of every endpoint.
pub const PREFIX: &str = "/api/v1";

/// Endpoint paths below [`PREFIX`]. Methods are listed in `api/openapi.yaml`.
pub mod paths {
    #![allow(missing_docs)]
    pub const SETUP: &str = "/api/v1/setup";
    pub const LOGIN: &str = "/api/v1/auth/login";
    pub const LOGOUT: &str = "/api/v1/auth/logout";
    pub const PASSWORD: &str = "/api/v1/auth/password";
    pub const STATUS: &str = "/api/v1/status";
    pub const CONTROL: &str = "/api/v1/control";
    pub const CONFIG_BYPASS: &str = "/api/v1/config/bypass";
    pub const CONFIG_FROST: &str = "/api/v1/config/frost";
    pub const CONFIG_NETWORK: &str = "/api/v1/config/network";
    pub const CONFIG_MQTT: &str = "/api/v1/config/mqtt";
    pub const CONFIG_FANS: &str = "/api/v1/config/fans";
    pub const CONFIG_PROGRAMS: &str = "/api/v1/config/programs";
    pub const CONFIG_HOLIDAY: &str = "/api/v1/config/holiday";
    pub const CONFIG_TIME: &str = "/api/v1/config/time";
    pub const CONFIG_FILTER: &str = "/api/v1/config/filter";
    pub const CONFIG_SCHEDULE: &str = "/api/v1/config/schedule";
    /// Followed by the profile number 1–4.
    pub const CONFIG_SCHEDULE_PROFILE: &str = "/api/v1/config/schedule/";
    /// `text/plain` INI download / upload.
    pub const CONFIG_EXPORT: &str = "/api/v1/config/export";
    pub const CONFIG_IMPORT: &str = "/api/v1/config/import";
    pub const FILTER_RESET: &str = "/api/v1/filter/reset";
    /// `text/plain` RAM log.
    pub const LOG: &str = "/api/v1/log";
    pub const SYSTEM: &str = "/api/v1/system";
    pub const SYSTEM_DETAILS: &str = "/api/v1/system/details";
    pub const SYSTEM_TIME: &str = "/api/v1/system/time";
    pub const REBOOT: &str = "/api/v1/system/reboot";
    /// Today's day record so far (`application/octet-stream`, `openkwl_app::history`).
    pub const HISTORY_TODAY: &str = "/api/v1/history/today";
    /// Followed by `YYYY-MM-DD` (one stored day record) or `YYYY-MM` (all of the month).
    pub const HISTORY_DAYS: &str = "/api/v1/history/days/";
}

/// Semantic checks beyond what deserialisation guarantees.
pub trait Validate {
    /// Returns the first offending field.
    fn validate(&self) -> Result<(), FieldError>;
}

pub(crate) fn check(ok: bool, field: &'static str, reason: &'static str) -> Result<(), FieldError> {
    if ok { Ok(()) } else { Err(FieldError { field, reason }) }
}

pub(crate) fn check_finite_range(value: f32, min: f32, max: f32, field: &'static str) -> Result<(), FieldError> {
    check(value.is_finite() && (min..=max).contains(&value), field, "out of range")
}

#[cfg(test)]
pub(crate) mod test_util {
    use serde::{Serialize, de::DeserializeOwned};

    /// Serialises `value`, checks the size bound, parses it back and returns the JSON text.
    pub fn round_trip<T, const N: usize>(value: &T) -> heapless::String<N>
    where
        T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let json: heapless::String<N> = serde_json_core::to_string(value).expect("fits in the documented maximum");
        let mut scratch = [0u8; 256];
        let (back, used): (T, usize) = serde_json_core::from_slice_escaped(json.as_bytes(), &mut scratch).expect("parses back");
        assert_eq!(used, json.len());
        assert_eq!(&back, value, "round trip of {json}");
        json
    }

    /// Parses a request body like the firmware does (with unescaping).
    pub fn parse<T: DeserializeOwned>(json: &str) -> Result<T, serde_json_core::de::Error> {
        let mut scratch = [0u8; 256];
        serde_json_core::from_slice_escaped(json.as_bytes(), &mut scratch).map(|(v, _)| v)
    }
}
