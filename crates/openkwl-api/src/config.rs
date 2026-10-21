//! `GET`/`PUT /config/{bypass,frost,network,mqtt}`. `PUT` replaces the whole object.

use core::net::Ipv4Addr;
use core::str::FromStr;

use openkwl_core::{bypass::BypassParams, clock, frost::FrostParams};
use serde::{Deserialize, Serialize};

use crate::{
    FieldError, Validate,
    auth::Password,
    check, check_finite_range,
    control::{MAX_LEVEL, MAX_PROGRAM_MIN, MIN_PROGRAM_MIN},
    time::{Date, MonthDay},
};

/// Dotted-quad IPv4 address, at most `255.255.255.255`.
pub type Ipv4String = heapless::String<15>;

// ---------------------------------------------------------------------------------------
// Bypass

/// Bypass actuator variants the stock firmware supports. The fitted type has
/// not been confirmed yet. Stock type 3 is not understood and therefore not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActuatorType {
    /// Type 1: analog actuator on PE13 (TIM1_CH3 0 % / 100 %).
    Analog,
    /// Type 2: 2-wire motor, PD10 = open, PD11 = close.
    Motor,
    /// Type 4: as `motor` with reversed polarity.
    MotorReversed,
}

/// Summer bypass settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BypassConfig {
    /// When false the bypass stays closed.
    pub enabled: bool,
    /// Room/extract target temperature (stock `v01035`).
    pub target_c: f32,
    /// Minimum outdoor temperature (stock `v01036`).
    pub min_outdoor_c: f32,
    /// Stock "mode 2": close when outdoor air gets more than 2 K warmer than extract air.
    pub close_if_outdoor_warmer: bool,
    /// Fitted actuator type.
    pub actuator: ActuatorType,
    /// Actuator run time for a full stroke.
    pub run_time_s: u16,
    /// Months in which the bypass may open (stock `v02120…v02129`); `null` = all year.
    /// Outside the window, or while the clock is not set, the bypass stays closed.
    pub season: Option<Season>,
    /// Stock "offset supply air temp." `v01037`, 0–10 K. Stored for a lossless import; not
    /// used by the controller yet (its stock meaning is unknown).
    pub supply_offset_k: f32,
}

/// Inclusive date window; may wrap over New Year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Season {
    /// First day, `"MM-DD"`.
    pub start: MonthDay,
    /// Last day, `"MM-DD"`.
    pub end: MonthDay,
}

/// Upper bound of the encoded [`BypassConfig`].
pub const BYPASS_JSON_MAX: usize = 256;

impl BypassConfig {
    /// Whether the season allows opening today (`None` = clock not set).
    pub fn season_allows(&self, today: Option<clock::Date>) -> bool {
        match (self.season, today) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(s), Some(d)) => clock::in_season(d, s.start.0, s.end.0),
        }
    }

    /// Parameters for [`openkwl_core::bypass::decide`].
    pub const fn params(&self) -> BypassParams {
        BypassParams {
            target_c: self.target_c,
            min_outdoor_c: self.min_outdoor_c,
            close_if_outdoor_warmer: self.close_if_outdoor_warmer,
        }
    }
}

impl Validate for BypassConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check_finite_range(self.target_c, 10.0, 35.0, "target_c")?;
        check_finite_range(self.min_outdoor_c, 0.0, 30.0, "min_outdoor_c")?;
        check((5..=600).contains(&self.run_time_s), "run_time_s", "out of range")?;
        check_finite_range(self.supply_offset_k, 0.0, 10.0, "supply_offset_k")
    }
}

// ---------------------------------------------------------------------------------------
// Frost

/// Frost-protection parameters; same meaning and units as [`FrostParams`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FrostConfig {
    /// Outdoor temperature below which protection arms (stock −3.1 °C).
    pub arm_below_c: f32,
    /// Efficiency below which icing is assumed (stock 58 %).
    pub entry_pct: f32,
    /// "Not recovering" threshold, used as `hold_pct − 0.1` (stock 60 %).
    pub hold_pct: f32,
    /// Efficiency that ends protection after a test run (stock 60 %).
    pub exit_pct: f32,
    /// Time η stays low before the supply fan stops (stock 900 s).
    pub boost_delay_s: u32,
    /// Supply-fan off time per defrost (stock 10 800 s).
    pub defrost_s: u32,
    /// Supply-fan test-run time (stock 900 s).
    pub test_run_s: u32,
    /// Extra exhaust-fan voltage while protecting (stock 1.2 V).
    pub exhaust_boost_v: f32,
}

/// Upper bound of the encoded [`FrostConfig`].
pub const FROST_JSON_MAX: usize = 224;

impl From<FrostParams> for FrostConfig {
    fn from(p: FrostParams) -> Self {
        Self {
            arm_below_c: p.arm_below_c,
            entry_pct: p.entry_pct,
            hold_pct: p.hold_pct,
            exit_pct: p.exit_pct,
            boost_delay_s: p.boost_delay_s,
            defrost_s: p.defrost_s,
            test_run_s: p.test_run_s,
            exhaust_boost_v: p.exhaust_boost_v,
        }
    }
}

impl From<FrostConfig> for FrostParams {
    fn from(c: FrostConfig) -> Self {
        Self {
            arm_below_c: c.arm_below_c,
            entry_pct: c.entry_pct,
            hold_pct: c.hold_pct,
            exit_pct: c.exit_pct,
            boost_delay_s: c.boost_delay_s,
            defrost_s: c.defrost_s,
            test_run_s: c.test_run_s,
            exhaust_boost_v: c.exhaust_boost_v,
        }
    }
}

impl Validate for FrostConfig {
    fn validate(&self) -> Result<(), FieldError> {
        // Ranges around the stock values: defrosting stops the supply fan while the exhaust fan
        // runs (under-pressure in the house), so its duration and the boost stay bounded.
        check_finite_range(self.arm_below_c, -25.0, 0.0, "arm_below_c")?;
        check_finite_range(self.entry_pct, 0.0, 100.0, "entry_pct")?;
        check_finite_range(self.hold_pct, 0.0, 100.0, "hold_pct")?;
        check_finite_range(self.exit_pct, 0.0, 100.0, "exit_pct")?;
        check(self.entry_pct <= self.hold_pct, "hold_pct", "below entry_pct")?;
        check((60..=3600).contains(&self.boost_delay_s), "boost_delay_s", "out of range")?;
        check((600..=4 * 3600).contains(&self.defrost_s), "defrost_s", "out of range")?;
        check((60..=3600).contains(&self.test_run_s), "test_run_s", "out of range")?;
        check_finite_range(self.exhaust_boost_v, 0.0, 2.0, "exhaust_boost_v")
    }
}

// ---------------------------------------------------------------------------------------
// Network

/// Takes effect after `POST /system/reboot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Use DHCP; the static fields below are then ignored.
    pub dhcp: bool,
    /// RFC 1123 label, also used for DHCP and mDNS.
    pub hostname: heapless::String<32>,
    /// Static address; required when `dhcp` is false.
    pub address: Option<Ipv4String>,
    /// Static netmask (contiguous); required when `dhcp` is false.
    pub netmask: Option<Ipv4String>,
    /// Default gateway, must lie in the subnet.
    pub gateway: Option<Ipv4String>,
    /// DNS server.
    pub dns: Option<Ipv4String>,
}

/// Upper bound of the encoded [`NetworkConfig`].
pub const NETWORK_JSON_MAX: usize = 192;

fn parse_ip(s: &Option<Ipv4String>, field: &'static str, required: bool) -> Result<Option<Ipv4Addr>, FieldError> {
    match s {
        None => check(!required, field, "required").map(|()| None),
        Some(s) => Ipv4Addr::from_str(s).map(Some).map_err(|_| FieldError { field, reason: "not an IPv4 address" }),
    }
}

/// RFC 1123 host name label: letters, digits and inner hyphens.
fn valid_hostname(h: &str) -> bool {
    let b = h.as_bytes();
    !b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-') && b[0] != b'-' && b[b.len() - 1] != b'-'
}

impl Validate for NetworkConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check(valid_hostname(&self.hostname), "hostname", "invalid host name")?;
        let fixed = !self.dhcp;
        let address = parse_ip(&self.address, "address", fixed)?;
        let netmask = parse_ip(&self.netmask, "netmask", fixed)?;
        let gateway = parse_ip(&self.gateway, "gateway", false)?;
        parse_ip(&self.dns, "dns", false)?;
        if let Some(mask) = netmask {
            let bits = u32::from(mask);
            check(bits != 0 && (!bits).wrapping_add(1) & !bits == 0, "netmask", "not contiguous")?;
            if let (Some(a), Some(g)) = (address, gateway) {
                check(u32::from(a) & bits == u32::from(g) & bits, "gateway", "outside subnet")?;
            }
        }
        if let Some(a) = address {
            check(!a.is_unspecified() && !a.is_broadcast() && !a.is_multicast(), "address", "not a host address")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Fans

/// Level → voltage table and estimated air flow (`/config/fans`). Stock `v00012–v00019`;
/// this unit: 5.0 / 6.0 / 7.0 / 9.0 V for both fans.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FanConfig {
    /// Supply fan voltage for levels 1–4, 0–10 V (below 1.65 V the fan is off).
    pub supply_volts: [f32; 4],
    /// Extract/exhaust fan voltage for levels 1–4.
    pub extract_volts: [f32; 4],
    /// Measured supply air flow per level in m³/h (e.g. from commissioning); 0 = estimate it
    /// from the fan speed with `m3h_per_krpm`.
    pub airflow_m3h: [u16; 4],
    /// Air flow per 1000 rpm for the speed-based estimate ([`openkwl_core::airflow`]);
    /// 0 = no estimate. Default for the KWL EC 300 W: 118.
    pub m3h_per_krpm: u16,
}

/// Upper bound of the encoded [`FanConfig`].
pub const FANS_JSON_MAX: usize = 224;

impl FanConfig {
    /// Voltages of a stock KWL EC 300 W R (`tools/openkwl-import/testdata`); air flow not yet known.
    pub const STOCK: Self = Self {
        supply_volts: [5.0, 6.0, 7.0, 9.0],
        extract_volts: [5.0, 6.0, 7.0, 9.0],
        airflow_m3h: [0; 4],
        m3h_per_krpm: openkwl_core::airflow::KWL_EC_300_M3H_PER_KRPM,
    };

    /// `(supply, extract)` volts for a level; level 0 is off.
    pub fn volts(&self, level: u8) -> (f32, f32) {
        match level {
            1..=MAX_LEVEL => (self.supply_volts[usize::from(level) - 1], self.extract_volts[usize::from(level) - 1]),
            _ => (0.0, 0.0),
        }
    }
}

impl Validate for FanConfig {
    fn validate(&self) -> Result<(), FieldError> {
        for v in self.supply_volts {
            check_finite_range(v, 0.0, 10.0, "supply_volts")?;
        }
        for v in self.extract_volts {
            check_finite_range(v, 0.0, 10.0, "extract_volts")?;
        }
        check(self.airflow_m3h.iter().all(|&f| f <= 1000), "airflow_m3h", "out of range")?;
        check(self.m3h_per_krpm <= 1000, "m3h_per_krpm", "out of range")
    }
}

// ---------------------------------------------------------------------------------------
// Programs

/// Defaults for party/quiet when `PATCH /control` omits them (`/config/programs`).
/// Stock on this unit: booster 150 min at level 4, whisper 180 min at level 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramConfig {
    /// Party level, 0–4.
    pub party_level: u8,
    /// Party duration, 5–180 min.
    pub party_min: u16,
    /// Quiet level, 0–4.
    pub quiet_level: u8,
    /// Quiet duration, 5–180 min.
    pub quiet_min: u16,
}

/// Upper bound of the encoded [`ProgramConfig`].
pub const PROGRAMS_JSON_MAX: usize = 96;

impl Validate for ProgramConfig {
    fn validate(&self) -> Result<(), FieldError> {
        let minutes = MIN_PROGRAM_MIN..=MAX_PROGRAM_MIN;
        check(self.party_level <= MAX_LEVEL, "party_level", "out of range")?;
        check(minutes.contains(&self.party_min), "party_min", "out of range")?;
        check(self.quiet_level <= MAX_LEVEL, "quiet_level", "out of range")?;
        check(minutes.contains(&self.quiet_min), "quiet_min", "out of range")
    }
}

// ---------------------------------------------------------------------------------------
// Holiday

/// Holiday program (`/config/holiday`, stock `v00601–v00606`): between `start` and `end`
/// the fans run at `level` for `active_min` every `interval_h` hours and are off otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayConfig {
    /// Program armed; it only acts between the dates.
    pub enabled: bool,
    /// First day, `"YYYY-MM-DD"`.
    pub start: Date,
    /// Last day (inclusive).
    pub end: Date,
    /// Level during a burst, 0–4.
    pub level: u8,
    /// Burst interval, 1–24 h.
    pub interval_h: u8,
    /// Burst length, 5–300 min, at most the interval.
    pub active_min: u16,
}

/// Upper bound of the encoded [`HolidayConfig`].
pub const HOLIDAY_JSON_MAX: usize = 128;

impl HolidayConfig {
    /// Core holiday parameters, `None` when disabled.
    pub fn program(&self) -> Option<openkwl_core::holiday::Holiday> {
        self.enabled.then_some(openkwl_core::holiday::Holiday {
            start: self.start.0,
            end: self.end.0,
            level: self.level,
            interval_min: u16::from(self.interval_h) * 60,
            active_min: self.active_min,
        })
    }
}

impl Validate for HolidayConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check(self.start <= self.end, "end", "before start")?;
        check(self.level <= MAX_LEVEL, "level", "out of range")?;
        check((1..=24).contains(&self.interval_h), "interval_h", "out of range")?;
        check((5..=300).contains(&self.active_min), "active_min", "out of range")?;
        check(self.active_min <= u16::from(self.interval_h) * 60, "active_min", "longer than interval")
    }
}

// ---------------------------------------------------------------------------------------
// Time

/// Clock settings (`/config/time`). The RTC is synchronised by SNTP: at boot, then hourly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeConfig {
    /// Synchronise the clock via SNTP (default on). When off, set it with `POST /system/time`.
    pub ntp_enabled: bool,
    /// SNTP server host name or IPv4 address. Empty: the router (the DHCP gateway; most home
    /// routers answer NTP), or [`DEFAULT_NTP_SERVER`] if it doesn't answer or the address is static.
    pub ntp_server: heapless::String<64>,
    /// Standard-time offset from UTC in minutes (CET = 60).
    pub utc_offset_min: i16,
    /// Apply EU summer time (last Sunday of March to last Sunday of October).
    pub eu_dst: bool,
}

/// Upper bound of the encoded [`TimeConfig`].
pub const TIME_JSON_MAX: usize = 208;

/// SNTP server used when none is configured and DHCP offers none.
pub const DEFAULT_NTP_SERVER: &str = "pool.ntp.org";

impl TimeConfig {
    /// SNTP on, server from the router or the pool, Central European Time.
    pub fn default_cet() -> Self {
        Self { ntp_enabled: true, ntp_server: heapless::String::new(), utc_offset_min: 60, eu_dst: true }
    }

    /// Server to query: configured, else `dhcp_offered` (the platform passes the router, or a
    /// DHCP option 42 server where it has one), else [`DEFAULT_NTP_SERVER`].
    pub fn server<'a>(&'a self, dhcp_offered: Option<&'a str>) -> Option<&'a str> {
        if !self.ntp_enabled {
            return None;
        }
        Some(match (self.ntp_server.as_str(), dhcp_offered) {
            ("", Some(d)) if !d.is_empty() => d,
            ("", _) => DEFAULT_NTP_SERVER,
            (s, _) => s,
        })
    }

    /// Core time zone.
    pub const fn zone(&self) -> clock::TimeZone {
        clock::TimeZone { utc_offset_min: self.utc_offset_min, eu_dst: self.eu_dst }
    }
}

impl Validate for TimeConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check((-720..=840).contains(&self.utc_offset_min), "utc_offset_min", "out of range")?;
        // Stored in CONFIG.INI: no control characters either (`ini_safe`).
        check(
            ini_safe(&self.ntp_server) && !self.ntp_server.contains(|c: char| c.is_whitespace() || c == '/'),
            "ntp_server",
            "invalid host",
        )
    }
}

// ---------------------------------------------------------------------------------------
// Filter

/// Filter-change reminder (`/config/filter`, stock `v01032`). Reset with `POST /filter/reset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterConfig {
    /// Fan running time between filter changes, in 30-day months; 0 disables the reminder.
    pub interval_months: u8,
}

impl Validate for FilterConfig {
    fn validate(&self) -> Result<(), FieldError> {
        check(self.interval_months <= 24, "interval_months", "out of range")
    }
}

// ---------------------------------------------------------------------------------------
// MQTT

/// MQTT 3.1.1 guarantees client ids up to 23 characters.
pub type ClientId = heapless::String<23>;
/// MQTT topic prefix.
pub type Topic = heapless::String<48>;

/// `GET /config/mqtt`. The password is never returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttConfig {
    /// Connect to the broker.
    pub enabled: bool,
    /// Broker host name or IPv4 address.
    pub host: heapless::String<64>,
    /// Broker port (1883 for plain MQTT).
    pub port: u16,
    /// Empty for anonymous access.
    pub username: heapless::String<32>,
    /// A password is stored.
    pub password_set: bool,
    /// MQTT client id; also the Home Assistant node id.
    pub client_id: ClientId,
    /// State and command topics live below this (default `openkwl`).
    pub base_topic: Topic,
    /// Publish Home Assistant discovery messages.
    pub discovery: bool,
    /// Home Assistant discovery prefix, normally `homeassistant`.
    pub discovery_prefix: Topic,
}

/// Upper bound of the encoded [`MqttConfig`].
pub const MQTT_JSON_MAX: usize = 576;

/// `PUT /config/mqtt`. `password`: omitted or `null` keeps the stored one, `""` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttConfigUpdate {
    /// See [`MqttConfig::enabled`].
    pub enabled: bool,
    /// See [`MqttConfig::host`].
    pub host: heapless::String<64>,
    /// See [`MqttConfig::port`].
    pub port: u16,
    /// See [`MqttConfig::username`].
    pub username: heapless::String<32>,
    /// `None` keeps the stored password, `""` clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    /// See [`MqttConfig::client_id`].
    pub client_id: ClientId,
    /// See [`MqttConfig::base_topic`].
    pub base_topic: Topic,
    /// See [`MqttConfig::discovery`].
    pub discovery: bool,
    /// See [`MqttConfig::discovery_prefix`].
    pub discovery_prefix: Topic,
}

/// Text that `CONFIG.INI` keeps as it is: no line breaks or other control characters (they
/// would add lines to the file), no spaces at either end (trimmed when read), no ` #` (starts
/// a comment there).
pub fn ini_safe(s: &str) -> bool {
    // Byte pairs, not `str::contains`: a substring search pulls in about 1 KB of code.
    secret_safe(s) && !s.as_bytes().windows(2).any(|w| w[1] == b'#' && (w[0] == b' ' || w[0] == b'\t'))
}

/// Text that `SECRETS.INI` keeps as it is (it has no inline comments). ASCII checks on purpose:
/// line breaks and the INI's trimmed spaces are all ASCII, and Unicode tables cost flash.
pub fn secret_safe(s: &str) -> bool {
    let b = s.as_bytes();
    !b.iter().any(|&c| c < b' ' || c == 0x7f) && b.first() != Some(&b' ') && b.last() != Some(&b' ')
}

/// Topic prefix without wildcards, empty levels or a leading/trailing `/`.
pub fn valid_topic_prefix(t: &str) -> bool {
    !t.is_empty() && t.split('/').all(|level| !level.is_empty() && !level.contains(['+', '#', '\0']))
}

impl Validate for MqttConfigUpdate {
    fn validate(&self) -> Result<(), FieldError> {
        // Stored even while MQTT is off, so always checked: a line break here would add
        // settings to CONFIG.INI.
        const STORABLE: &str = "invalid characters";
        for (field, v) in [
            ("host", &*self.host),
            ("username", &self.username),
            ("client_id", &self.client_id),
            ("base_topic", &self.base_topic),
            ("discovery_prefix", &self.discovery_prefix),
        ] {
            check(ini_safe(v), field, STORABLE)?;
        }
        // SECRETS.INI has no inline comments, but trims and ends at a line break.
        if let Some(p) = &self.password {
            check(secret_safe(p), "password", STORABLE)?;
        }
        if !self.enabled {
            return Ok(());
        }
        check(!self.host.is_empty() && !self.host.contains(|c: char| c.is_whitespace() || c == '/'), "host", "invalid host")?;
        check(self.port != 0, "port", "out of range")?;
        check(
            !self.client_id.is_empty() && self.client_id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "client_id",
            "use letters, digits, - and _",
        )?;
        check(valid_topic_prefix(&self.base_topic), "base_topic", "invalid topic")?;
        check(valid_topic_prefix(&self.discovery_prefix), "discovery_prefix", "invalid topic")
    }
}

impl MqttConfigUpdate {
    /// The stored view after applying this update.
    pub fn applied(&self, previous_password_set: bool) -> MqttConfig {
        MqttConfig {
            enabled: self.enabled,
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password_set: match &self.password {
                None => previous_password_set,
                Some(p) => !p.is_empty(),
            },
            client_id: self.client_id.clone(),
            base_topic: self.base_topic.clone(),
            discovery: self.discovery,
            discovery_prefix: self.discovery_prefix.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{parse, round_trip};

    #[test]
    fn bypass_round_trip_and_params() {
        let c = BypassConfig {
            enabled: true,
            target_c: 23.0,
            min_outdoor_c: 15.0,
            close_if_outdoor_warmer: true,
            actuator: ActuatorType::MotorReversed,
            run_time_s: 120,
            season: Some(Season {
                start: MonthDay(clock::MonthDay::new(3, 1).unwrap()),
                end: MonthDay(clock::MonthDay::new(10, 2).unwrap()),
            }),
            supply_offset_k: 4.0,
        };
        let json = round_trip::<_, BYPASS_JSON_MAX>(&c);
        assert!(json.contains(r#""season":{"start":"03-01","end":"10-02"}"#), "{json}");
        let worst =
            BypassConfig { target_c: -1.234_567_9e-12, min_outdoor_c: -1.234_567_9e-12, supply_offset_k: -1.234_567_9e-12, ..c };
        round_trip::<_, BYPASS_JSON_MAX>(&worst);
        let d = |m, day| clock::Date::new(2026, m, day);
        assert!(c.season_allows(d(7, 1)) && !c.season_allows(d(11, 1)) && !c.season_allows(None));
        assert!(BypassConfig { season: None, ..c }.season_allows(None), "no window: clock not needed");
        assert!(json.contains(r#""actuator":"motor_reversed""#));
        assert!(c.validate().is_ok());
        assert!(openkwl_core::bypass::decide(false, &c.params(), 25.0, 18.0));
        assert_eq!(BypassConfig { target_c: f32::NAN, ..c }.validate().unwrap_err().field, "target_c");
        assert_eq!(BypassConfig { run_time_s: 0, ..c }.validate().unwrap_err().field, "run_time_s");
        assert_eq!(BypassConfig { supply_offset_k: 11.0, ..c }.validate().unwrap_err().field, "supply_offset_k");
    }

    #[test]
    fn frost_matches_core_params() {
        let c = FrostConfig::from(FrostParams::STOCK);
        assert!(c.validate().is_ok());
        assert_eq!(FrostParams::from(c), FrostParams::STOCK);
        let json = round_trip::<_, FROST_JSON_MAX>(&c);
        assert!(json.starts_with(r#"{"arm_below_c":-3.1,"entry_pct":58.0,"#), "{json}");
        let worst = FrostConfig {
            arm_below_c: -1.234_567_9e-12,
            entry_pct: -1.234_567_9e-12,
            hold_pct: -1.234_567_9e-12,
            exit_pct: -1.234_567_9e-12,
            boost_delay_s: u32::MAX,
            defrost_s: u32::MAX,
            test_run_s: u32::MAX,
            exhaust_boost_v: -1.234_567_9e-12,
        };
        round_trip::<_, FROST_JSON_MAX>(&worst);
        assert_eq!(FrostConfig { entry_pct: 70.0, ..c }.validate().unwrap_err().field, "hold_pct");
        assert_eq!(FrostConfig { defrost_s: 0, ..c }.validate().unwrap_err().field, "defrost_s");
        let field = |f: FrostConfig| f.validate().unwrap_err().field;
        assert_eq!(field(FrostConfig { defrost_s: 599, ..c }), "defrost_s");
        assert_eq!(field(FrostConfig { defrost_s: 14_401, ..c }), "defrost_s");
        assert_eq!(field(FrostConfig { boost_delay_s: 3601, ..c }), "boost_delay_s");
        assert_eq!(field(FrostConfig { test_run_s: 3601, ..c }), "test_run_s");
        assert_eq!(field(FrostConfig { exhaust_boost_v: 2.01, ..c }), "exhaust_boost_v");
        assert_eq!(field(FrostConfig { arm_below_c: 0.5, ..c }), "arm_below_c");
        let edges =
            FrostConfig { arm_below_c: 0.0, boost_delay_s: 3600, defrost_s: 14_400, test_run_s: 60, exhaust_boost_v: 2.0, ..c };
        assert!(edges.validate().is_ok());
    }

    #[test]
    fn network_validation() {
        let n: NetworkConfig = parse(
            r#"{"dhcp":false,"hostname":"kwl","address":"192.168.10.20","netmask":"255.255.255.0","gateway":"192.168.10.1","dns":"192.168.10.1"}"#,
        )
        .unwrap();
        assert!(n.validate().is_ok());
        round_trip::<_, NETWORK_JSON_MAX>(&n);
        let worst = NetworkConfig {
            hostname: "k".repeat_into(), // validated: letters, digits and '-' only
            address: Some("255.255.255.255".try_into().unwrap()),
            netmask: Some("255.255.255.255".try_into().unwrap()),
            gateway: Some("255.255.255.255".try_into().unwrap()),
            dns: Some("255.255.255.255".try_into().unwrap()),
            ..n.clone()
        };
        round_trip::<_, NETWORK_JSON_MAX>(&worst);

        let dhcp: NetworkConfig =
            parse(r#"{"dhcp":true,"hostname":"kwl","address":null,"netmask":null,"gateway":null,"dns":null}"#).unwrap();
        assert!(dhcp.validate().is_ok());

        let field = |n: NetworkConfig| n.validate().unwrap_err().field;
        assert_eq!(field(NetworkConfig { address: None, ..n.clone() }), "address");
        assert_eq!(field(NetworkConfig { address: Some("192.168.10".try_into().unwrap()), ..n.clone() }), "address");
        assert_eq!(field(NetworkConfig { netmask: Some("255.0.255.0".try_into().unwrap()), ..n.clone() }), "netmask");
        assert_eq!(field(NetworkConfig { gateway: Some("10.0.0.1".try_into().unwrap()), ..n.clone() }), "gateway");
        assert_eq!(field(NetworkConfig { hostname: "-kwl".try_into().unwrap(), ..n.clone() }), "hostname");
        assert_eq!(field(NetworkConfig { hostname: "".try_into().unwrap(), ..n }), "hostname");
        assert!(parse::<NetworkConfig>(r#"{"dhcp":true,"hostname":"kwl","address":"1234.123.123.123x"}"#).is_err());
    }

    trait RepeatInto {
        fn repeat_into<const N: usize>(&self) -> heapless::String<N>;
    }
    impl RepeatInto for str {
        fn repeat_into<const N: usize>(&self) -> heapless::String<N> {
            let mut s = heapless::String::new();
            while s.push_str(self).is_ok() {}
            s
        }
    }

    fn mqtt() -> MqttConfigUpdate {
        MqttConfigUpdate {
            enabled: true,
            host: "broker.lan".try_into().unwrap(),
            port: 1883,
            username: "kwl".try_into().unwrap(),
            password: Some("secret".try_into().unwrap()),
            client_id: "openkwl".try_into().unwrap(),
            base_topic: "openkwl".try_into().unwrap(),
            discovery: true,
            discovery_prefix: "homeassistant".try_into().unwrap(),
        }
    }

    #[test]
    fn mqtt_text_must_survive_config_ini() {
        // A line break would add lines to CONFIG.INI, " #" would cut the value, both even with
        // MQTT switched off.
        let off = |u: MqttConfigUpdate| MqttConfigUpdate { enabled: false, ..u };
        let bad = [
            MqttConfigUpdate { username: "x\n[time]\nntp_enabled = false".try_into().unwrap(), ..off(mqtt()) },
            MqttConfigUpdate { username: "home #2".try_into().unwrap(), ..mqtt() },
            MqttConfigUpdate { host: "broker.lan ".try_into().unwrap(), ..off(mqtt()) },
            MqttConfigUpdate { base_topic: "openkwl\r".try_into().unwrap(), ..mqtt() },
            MqttConfigUpdate { password: Some(" secret".try_into().unwrap()), ..mqtt() },
            MqttConfigUpdate { password: Some("sec\nret".try_into().unwrap()), ..mqtt() },
        ];
        for u in bad {
            assert!(u.validate().is_err(), "{u:?}");
        }
        assert!(
            MqttConfigUpdate { username: "home#2".try_into().unwrap(), password: Some("p #w d".try_into().unwrap()), ..mqtt() }
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn mqtt_round_trip_and_password_handling() {
        let u = mqtt();
        assert!(u.validate().is_ok());
        round_trip::<_, MQTT_JSON_MAX>(&u);

        let view = u.applied(false);
        assert!(view.password_set);
        let json = round_trip::<_, MQTT_JSON_MAX>(&view);
        assert!(!json.contains("secret"), "password never echoed");

        assert!(MqttConfigUpdate { password: None, ..mqtt() }.applied(true).password_set, "kept");
        let cleared = MqttConfigUpdate { password: Some(Password::new()), ..mqtt() };
        assert!(!cleared.applied(true).password_set);

        let keep: MqttConfigUpdate = parse(
            r#"{"enabled":true,"host":"h","port":1883,"username":"","client_id":"c","base_topic":"b","discovery":false,"discovery_prefix":"homeassistant"}"#,
        )
        .unwrap();
        assert_eq!(keep.password, None);

        // Every byte a `"` (escaped to two; control characters are refused by `ini_safe`):
        // allowed in every text field while MQTT is off.
        let worst = MqttConfig {
            enabled: false,
            host: "\"".repeat_into(),
            port: u16::MAX,
            username: "\\".repeat_into(),
            password_set: false,
            client_id: "\"".repeat_into(),
            base_topic: "\"".repeat_into(),
            discovery: false,
            discovery_prefix: "\"".repeat_into(),
        };
        assert_eq!(round_trip::<_, MQTT_JSON_MAX>(&worst).len(), MQTT_JSON_MAX, "the bound is exact");

        let worst_update = MqttConfigUpdate {
            host: worst.host.clone(),
            username: worst.username.clone(),
            password: Some("\"".repeat_into()),
            client_id: worst.client_id.clone(),
            base_topic: worst.base_topic.clone(),
            discovery_prefix: worst.discovery_prefix.clone(),
            ..mqtt()
        };
        assert!(MqttConfigUpdate { enabled: false, ..worst_update.clone() }.validate().is_ok());
        round_trip::<_, { crate::REQUEST_BODY_MAX }>(&worst_update);
    }

    #[test]
    fn mqtt_validation() {
        let field = |u: MqttConfigUpdate| u.validate().unwrap_err().field;
        assert_eq!(field(MqttConfigUpdate { host: "".try_into().unwrap(), ..mqtt() }), "host");
        assert_eq!(field(MqttConfigUpdate { port: 0, ..mqtt() }), "port");
        assert_eq!(field(MqttConfigUpdate { client_id: "a b".try_into().unwrap(), ..mqtt() }), "client_id");
        for bad in ["helios/#", "helios/+/x", "/helios", "helios/", "a//b"] {
            assert_eq!(field(MqttConfigUpdate { base_topic: bad.try_into().unwrap(), ..mqtt() }), "base_topic", "{bad}");
        }
        assert!(MqttConfigUpdate { enabled: false, host: "".try_into().unwrap(), ..mqtt() }.validate().is_ok());
    }

    #[test]
    fn fans() {
        let c = FanConfig::STOCK;
        assert!(c.validate().is_ok());
        assert_eq!(c.volts(2), (6.0, 6.0));
        assert_eq!(c.volts(0), (0.0, 0.0));
        assert_eq!(c.volts(9), (0.0, 0.0));
        let json = round_trip::<_, FANS_JSON_MAX>(&c);
        assert!(json.starts_with(r#"{"supply_volts":[5.0,6.0,7.0,9.0],"#), "{json}");
        let x = -1.234_567_9e-12;
        round_trip::<_, FANS_JSON_MAX>(&FanConfig {
            supply_volts: [x; 4],
            extract_volts: [x; 4],
            airflow_m3h: [u16::MAX; 4],
            m3h_per_krpm: u16::MAX,
        });
        assert_eq!(FanConfig { supply_volts: [11.0, 6.0, 7.0, 9.0], ..c }.validate().unwrap_err().field, "supply_volts");
        assert_eq!(FanConfig { airflow_m3h: [1001, 0, 0, 0], ..c }.validate().unwrap_err().field, "airflow_m3h");
    }

    #[test]
    fn programs() {
        let p = ProgramConfig { party_level: 4, party_min: 150, quiet_level: 0, quiet_min: 180 };
        assert!(p.validate().is_ok());
        round_trip::<_, PROGRAMS_JSON_MAX>(&p);
        round_trip::<_, PROGRAMS_JSON_MAX>(&ProgramConfig {
            party_level: 255,
            party_min: u16::MAX,
            quiet_level: 255,
            quiet_min: u16::MAX,
        });
        assert_eq!(ProgramConfig { quiet_min: 4, ..p }.validate().unwrap_err().field, "quiet_min");
        assert_eq!(ProgramConfig { party_level: 5, ..p }.validate().unwrap_err().field, "party_level");
    }

    #[test]
    fn holiday() {
        let h: HolidayConfig =
            parse(r#"{"enabled":true,"start":"2024-07-13","end":"2024-07-19","level":1,"interval_h":2,"active_min":60}"#).unwrap();
        assert!(h.validate().is_ok());
        round_trip::<_, HOLIDAY_JSON_MAX>(&h);
        let p = h.program().unwrap();
        assert_eq!((p.interval_min, p.active_min), (120, 60));
        assert!(HolidayConfig { enabled: false, ..h }.program().is_none());
        assert_eq!(
            HolidayConfig { end: Date(clock::Date::new(2024, 7, 12).unwrap()), ..h }
                .validate()
                .unwrap_err()
                .field,
            "end"
        );
        assert_eq!(HolidayConfig { interval_h: 1, active_min: 61, ..h }.validate().unwrap_err().reason, "longer than interval");
        assert_eq!(HolidayConfig { interval_h: 25, ..h }.validate().unwrap_err().field, "interval_h");
        assert!(
            parse::<HolidayConfig>(
                r#"{"enabled":true,"start":"2024-13-01","end":"2024-07-19","level":1,"interval_h":2,"active_min":60}"#
            )
            .is_err()
        );
    }

    #[test]
    fn time_and_filter() {
        let t = TimeConfig { ntp_server: "time.example.net".try_into().unwrap(), ..TimeConfig::default_cet() };
        assert!(t.validate().is_ok());
        assert_eq!(t.zone(), clock::TimeZone::CET);
        assert_eq!(t.server(Some("192.168.1.1")), Some("time.example.net"), "configured wins");
        let d = TimeConfig::default_cet();
        assert_eq!(d.server(Some("192.168.1.1")), Some("192.168.1.1"), "DHCP option 42");
        assert_eq!(d.server(None), Some(DEFAULT_NTP_SERVER));
        assert_eq!(TimeConfig { ntp_enabled: false, ..d }.server(None), None);
        round_trip::<_, TIME_JSON_MAX>(&t);
        // Control characters are refused (CONFIG.INI); `"` passes the host check and is escaped
        // to two bytes.
        assert_eq!(
            TimeConfig { ntp_server: "a\u{1}b".try_into().unwrap(), ..t.clone() }.validate().unwrap_err().field,
            "ntp_server"
        );
        let worst = TimeConfig { ntp_enabled: false, ntp_server: "\"".repeat_into(), utc_offset_min: i16::MIN, eu_dst: false };
        assert_eq!(TimeConfig { utc_offset_min: 0, ..worst.clone() }.validate(), Ok(()));
        let n = round_trip::<_, TIME_JSON_MAX>(&worst).len();
        assert!(n > TIME_JSON_MAX - 8, "worst case {n} bytes");
        assert_eq!(TimeConfig { utc_offset_min: 900, ..t.clone() }.validate().unwrap_err().field, "utc_offset_min");
        assert_eq!(TimeConfig { ntp_server: "a b".try_into().unwrap(), ..t }.validate().unwrap_err().field, "ntp_server");
        assert!(FilterConfig { interval_months: 3 }.validate().is_ok());
        assert!(FilterConfig { interval_months: 25 }.validate().is_err());
        round_trip::<_, 32>(&FilterConfig { interval_months: 255 });
    }
}
