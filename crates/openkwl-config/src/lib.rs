//! Persistent settings on the SD card (`docs/PLAN.md` §3.1).
//!
//! | File (in [`DIR`]) | Contents | Written |
//! |---|---|---|
//! | [`CONFIG_INI`] | All settings, human-readable INI ([`ini`]) | On change, debounced ([`schedule::Debounce`]) |
//! | [`CONFIG_BAK`] | Last copy that loaded cleanly | After a successful load that differs |
//! | [`SECRETS_INI`] | MQTT password; never served over HTTP | On change |
//! | [`AUTH_BIN`] | PBKDF2 password record + CRC ([`auth_file`]) | On password change |
//! | [`STATE_A`] / [`STATE_B`] | Runtime counters, alternating A/B ([`state`]) | Every 6 h, on reboot, on power fail |
//!
//! File names are 8.3 because `embedded-sdmmc` has no long-file-name support. The crate is
//! `no_std`; the firmware does the SD I/O.

#![no_std]

pub mod auth_file;
pub mod crc;
pub mod ini;
pub mod schedule;
pub mod state;
pub mod stock;

use openkwl_api::{
    ActuatorType, BypassConfig, DeviceIdentity, FanConfig, FilterConfig, FrostConfig, HolidayConfig, MqttConfig, NetworkConfig,
    ProgramConfig, ScheduleConfig, ScheduleProfile, TimeConfig, time::Date,
};
use openkwl_core::{clock, frost::FrostParams, schedule::PROFILES};

pub const DIR: &str = "HELIOS";
pub const CONFIG_INI: &str = "CONFIG.INI";
pub const CONFIG_BAK: &str = "CONFIG.BAK";
pub const SECRETS_INI: &str = "SECRETS.INI";
pub const SECRETS_BAK: &str = "SECRETS.BAK";
pub const AUTH_BIN: &str = "AUTH.BIN";
/// Each settings file has a backup that is written first with the same content, then the file
/// itself: a power cut while saving leaves one of the two complete (an interrupted write leaves
/// an empty file, and the loader falls back to the backup). `CONFIG.INI` stays the one to edit.
pub const AUTH_BAK: &str = "AUTH.BAK";
pub const STATE_A: &str = "STATEA.BIN";
pub const STATE_B: &str = "STATEB.BIN";
/// Optional one-shot password preseed in the card root; deleted after use.
pub const SETUP_INI: &str = "SETUP.INI";
/// Empty marker file in the card root: wipe `AUTH.BIN` and `AUTH.BAK` at boot (password reset).
pub const RESET_PW: &str = "RESETPW";
/// Largest `CONFIG.INI` the firmware reads (a full config with 4 schedules is ≈ 6 KB).
pub const CONFIG_MAX_LEN: usize = 12 * 1024;

/// Everything in `CONFIG.INI`.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub network: NetworkConfig,
    pub time: TimeConfig,
    /// `password_set` is not stored here; the firmware derives it from [`Secrets`].
    pub mqtt: MqttConfig,
    pub fans: FanConfig,
    pub programs: ProgramConfig,
    pub bypass: BypassConfig,
    pub frost: FrostConfig,
    pub holiday: HolidayConfig,
    pub filter: FilterConfig,
    pub schedule: ScheduleConfig,
    pub profiles: [ScheduleProfile; PROFILES],
    /// Factory identity and MAC, taken over from the stock unit; read-only in the UI.
    pub device: Device,
}

/// `[device]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub identity: DeviceIdentity,
    /// Ethernet MAC, `aa:bb:cc:dd:ee:ff`; empty until imported (the firmware then refuses to
    /// start the network rather than invent one).
    pub mac: heapless::String<17>,
}

/// Everything in `SECRETS.INI`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    /// Empty = no password.
    pub mqtt_password: openkwl_api::auth::Password,
}

fn s<const N: usize>(v: &str) -> heapless::String<N> {
    v.try_into().unwrap_or_default()
}

impl Default for Config {
    /// Defaults for a fresh unit: values read from a stock unit where known
    /// (`tools/openkwl-import/testdata`), conservative choices elsewhere. Import replaces them.
    fn default() -> Self {
        let all_day = |level| {
            let mut day = heapless::Vec::new();
            let _ = day.push(openkwl_api::Slot { from: openkwl_api::time::TimeOfDay(0), level });
            day
        };
        let profile = |level| ScheduleProfile { days: core::array::from_fn(|_| all_day(level)) };
        Self {
            network: NetworkConfig { dhcp: true, hostname: s("openkwl"), address: None, netmask: None, gateway: None, dns: None },
            time: TimeConfig::default_cet(),
            mqtt: MqttConfig {
                enabled: false,
                host: s(""),
                port: 1883,
                username: s(""),
                password_set: false,
                client_id: s("openkwl"),
                base_topic: s("openkwl"),
                discovery: true,
                discovery_prefix: s("homeassistant"),
            },
            fans: FanConfig::STOCK,
            programs: ProgramConfig { party_level: 4, party_min: 150, quiet_level: 0, quiet_min: 180 },
            bypass: BypassConfig {
                enabled: true,
                target_c: 22.0,
                min_outdoor_c: 7.0,
                close_if_outdoor_warmer: true,
                // The fitted actuator type is unconfirmed (README bring-up checklist).
                actuator: ActuatorType::Motor,
                run_time_s: 120,
                season: None,
                supply_offset_k: 4.0,
            },
            frost: FrostConfig::from(FrostParams::STOCK),
            holiday: HolidayConfig {
                enabled: false,
                start: Date(clock::Date { year: 2026, month: 1, day: 1 }),
                end: Date(clock::Date { year: 2026, month: 1, day: 1 }),
                level: 1,
                interval_h: 2,
                active_min: 60,
            },
            filter: FilterConfig { interval_months: 3 },
            schedule: ScheduleConfig { active_profile: 1, auto_at_midnight: true, auto_return_min: None, fallback_level: 2 },
            profiles: [profile(2), profile(2), profile(2), profile(1), profile(1)],
            device: Device {
                // Unknown until imported from the stock unit (Home Assistant then shows a
                // generic model rather than the reference unit's).
                identity: DeviceIdentity {
                    model: s(""),
                    article: s(""),
                    serial: s(""),
                    production_code: s(""),
                    security_code: s(""),
                },
                mac: s(""),
            },
        }
    }
}
