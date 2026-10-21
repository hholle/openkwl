//! MQTT integration and Home Assistant discovery.
//!
//! Topic layout below the configured base topic (`MqttConfig::base_topic`, e.g. `openkwl`):
//!
//! | Topic | Direction | Payload |
//! |---|---|---|
//! | `<base>/availability` | device → broker, retained, also the last will | `online` / `offline` |
//! | `<base>/state` | device → broker, retained | [`State`] JSON |
//! | `<base>/fan/set` | broker → device | `ON` / `OFF` |
//! | `<base>/fan/level/set` | broker → device | `0`–`4` |
//! | `<base>/fan/preset/set` | broker → device | `auto` / `party` / `quiet` / `None` (manual) |
//! | `<base>/bypass/enabled/set` | broker → device | `ON` / `OFF` |
//! | `<base>/bypass/target/set` | broker → device | °C, e.g. `22.5` |
//! | `<base>/filter/reset` | broker → device | `PRESS` |
//! | `<base>/system/reboot` | broker → device | `PRESS` |
//!
//! Discovery messages go to `<discovery_prefix>/<component>/<node_id>/<object>/config`
//! (retained), one per entity, and use HA's `~` abbreviation for the base topic so the
//! definitions in [`ENTITIES`] are static. HA's single-message device discovery would need
//! a ~8 KB payload, too large for the device's buffers.
//!
//! Entity choice follows HA conventions for ventilation units (cf. the Vallox
//! integration): a `fan` for on/off, speed and presets; sensors with device classes;
//! `switch`/`number`/`button` for the few settings worth automating. MQTT has no `date`
//! platform, so the filter reminder is a `duration` sensor plus a reset button.

#![no_std]

mod json;
pub mod mqtt;

use core::fmt::Write as _;

use openkwl_api::{BypassConfig, ControlRequest, Fault, FrostState, Mode, Status, control::MAX_LEVEL};
use serde::Serialize;

pub type Topic = heapless::String<128>;

pub const ONLINE: &str = "online";
pub const OFFLINE: &str = "offline";
/// HA publishes `online` here after a restart; republish discovery then.
pub const HA_STATUS_TOPIC_SUFFIX: &str = "/status";

/// Largest discovery payload for maximum-length names (tested).
pub const DISCOVERY_JSON_MAX: usize = 1280;
/// Model shown in Home Assistant when the unit's model is not known (not imported).
pub const UNKNOWN_MODEL: &str = "KWL EC (easyControls)";
/// Largest state payload (tested).
pub const STATE_JSON_MAX: usize = 1088;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A topic or payload does not fit its buffer.
    TooLong,
    UnknownTopic,
    BadPayload,
}

/// Device identity and topic configuration, usually from `MqttConfig` and `SystemInfo`.
#[derive(Debug, Clone, Copy)]
pub struct Device<'a> {
    /// Unique per device; `[A-Za-z0-9_-]` (the MQTT client id).
    pub node_id: &'a str,
    /// Display name of the device in HA.
    pub name: &'a str,
    pub sw_version: &'a str,
    /// The unit's model (`DeviceIdentity::model`, imported from the stock settings); empty when
    /// unknown, then [`UNKNOWN_MODEL`] is shown.
    pub model: &'a str,
    /// `aa:bb:cc:dd:ee:ff`; lets HA merge the device with router integrations.
    pub mac: &'a str,
    /// Link to the web UI, e.g. `http://192.168.1.50/`.
    pub configuration_url: Option<&'a str>,
    pub base_topic: &'a str,
    pub discovery_prefix: &'a str,
}

fn topic(parts: core::fmt::Arguments<'_>) -> Result<Topic, Error> {
    let mut t = Topic::new();
    t.write_fmt(parts).map_err(|_| Error::TooLong)?;
    Ok(t)
}

impl Device<'_> {
    /// The model for HA's device card: the imported one, else [`UNKNOWN_MODEL`].
    fn model_or_unknown(&self) -> &str {
        match self.model.trim() {
            "" => UNKNOWN_MODEL,
            m => m,
        }
    }

    pub fn availability_topic(&self) -> Result<Topic, Error> {
        topic(format_args!("{}/availability", self.base_topic))
    }

    pub fn state_topic(&self) -> Result<Topic, Error> {
        topic(format_args!("{}/state", self.base_topic))
    }

    /// Subscription filters covering every command topic (the state topic is excluded so
    /// the device does not receive its own retained state).
    pub fn command_filters(&self) -> Result<[Topic; 4], Error> {
        Ok([
            topic(format_args!("{}/fan/#", self.base_topic))?,
            topic(format_args!("{}/bypass/#", self.base_topic))?,
            topic(format_args!("{}/filter/#", self.base_topic))?,
            topic(format_args!("{}/system/#", self.base_topic))?,
        ])
    }

    /// HA's birth-message topic.
    pub fn ha_status_topic(&self) -> Result<Topic, Error> {
        topic(format_args!("{}{}", self.discovery_prefix, HA_STATUS_TOPIC_SUFFIX))
    }

    pub fn discovery_topic(&self, e: &Entity) -> Result<Topic, Error> {
        topic(format_args!("{}/{}/{}/{}/config", self.discovery_prefix, e.component.as_str(), self.node_id, e.object_id))
    }
}

// ---------------------------------------------------------------------------------------
// State

/// Flat state document for HA templates. Unknown numbers are `null`; HA's MQTT sensor
/// treats the rendered `None` as "unknown".
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(Serialize))]
pub struct State {
    /// `ON` while level > 0.
    pub fan: OnOff,
    /// 0–4.
    pub level: u8,
    /// Fan preset: `auto`, `party`, `quiet`, or `None` (manual and holiday).
    pub preset: &'static str,
    /// Operating mode including holiday.
    pub mode: Mode,
    /// Minutes until manual mode returns to auto (rounded up), `null` if not pending.
    pub auto_return_min: Option<u32>,
    pub t_outdoor: Option<f32>,
    pub t_supply: Option<f32>,
    pub t_extract: Option<f32>,
    pub t_exhaust: Option<f32>,
    pub efficiency: Option<f32>,
    pub heat_w: Option<f32>,
    pub heat_kwh: f32,
    pub cool_kwh: f32,
    pub supply_rpm: u32,
    pub exhaust_rpm: u32,
    /// Measured per level or estimated from the speed (`openkwl_core::airflow`).
    pub supply_m3h: Option<f32>,
    pub exhaust_m3h: Option<f32>,
    pub supply_v: f32,
    pub exhaust_v: f32,
    pub bypass: OnOff,
    pub bypass_enabled: OnOff,
    pub bypass_target: f32,
    pub frost: OnOff,
    pub defrost: OnOff,
    pub frost_state: FrostState,
    pub filter_days: u32,
    pub filter_due: OnOff,
    pub op_hours: u32,
    pub problem: OnOff,
    pub faults: heapless::Vec<Fault, { openkwl_api::status::MAX_FAULTS }>,
}

/// How much a state differs from the last published one ([`State::change_from`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    /// Nothing worth sending (measurements within their dead band).
    None,
    /// Only a diagnostic value moved (fan speed, fan voltage): publish, rate-limited more.
    Diagnostic,
    /// A measurement moved noticeably: publish, but rate-limited.
    Measurement,
    /// Something switched (mode, level, bypass, frost, a fault, a counter step): publish now.
    State,
}

/// Dead bands: smaller moves are noise for Home Assistant's history.
const TEMP_K: f32 = 0.2;
const EFFICIENCY_PCT: f32 = 1.0;
const POWER_W: f32 = 20.0;
const ENERGY_KWH: f32 = 0.05;
const RPM: u32 = 50;
const AIRFLOW_M3H: f32 = 5.0;
const VOLTS: f32 = 0.1;

fn moved(a: Option<f32>, b: Option<f32>, band: f32) -> Option<Change> {
    match (a, b) {
        (Some(a), Some(b)) => ((a - b).abs() >= band).then_some(Change::Measurement),
        (None, None) => None,
        // A sensor appeared or disappeared.
        _ => Some(Change::State),
    }
}

impl State {
    /// Compares with the last published state: switching values count as [`Change::State`],
    /// measurements only beyond their dead band.
    pub fn change_from(&self, prev: &State) -> Change {
        let switched = self.fan != prev.fan
            || self.level != prev.level
            || self.preset != prev.preset
            || self.mode != prev.mode
            || self.auto_return_min != prev.auto_return_min
            || self.bypass != prev.bypass
            || self.bypass_enabled != prev.bypass_enabled
            || self.bypass_target != prev.bypass_target
            || self.frost != prev.frost
            || self.defrost != prev.defrost
            || self.frost_state != prev.frost_state
            || self.filter_days != prev.filter_days
            || self.filter_due != prev.filter_due
            || self.op_hours != prev.op_hours
            || self.problem != prev.problem
            || self.faults != prev.faults;
        if switched {
            return Change::State;
        }
        let some = |v: f32| Some(v);
        [
            moved(self.t_outdoor, prev.t_outdoor, TEMP_K),
            moved(self.t_supply, prev.t_supply, TEMP_K),
            moved(self.t_extract, prev.t_extract, TEMP_K),
            moved(self.t_exhaust, prev.t_exhaust, TEMP_K),
            moved(self.efficiency, prev.efficiency, EFFICIENCY_PCT),
            moved(self.heat_w, prev.heat_w, POWER_W),
            moved(some(self.heat_kwh), some(prev.heat_kwh), ENERGY_KWH),
            moved(some(self.cool_kwh), some(prev.cool_kwh), ENERGY_KWH),
            moved(self.supply_m3h, prev.supply_m3h, AIRFLOW_M3H),
            moved(self.exhaust_m3h, prev.exhaust_m3h, AIRFLOW_M3H),
            moved(some(self.supply_v), some(prev.supply_v), VOLTS).map(|_| Change::Diagnostic),
            moved(some(self.exhaust_v), some(prev.exhaust_v), VOLTS).map(|_| Change::Diagnostic),
            (self.supply_rpm.abs_diff(prev.supply_rpm) >= RPM).then_some(Change::Diagnostic),
            (self.exhaust_rpm.abs_diff(prev.exhaust_rpm) >= RPM).then_some(Change::Diagnostic),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(Change::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OnOff {
    #[serde(rename = "ON")]
    On,
    #[serde(rename = "OFF")]
    Off,
}

impl OnOff {
    pub const fn as_str(self) -> &'static str {
        match self {
            OnOff::On => "ON",
            OnOff::Off => "OFF",
        }
    }
}

impl From<bool> for OnOff {
    fn from(b: bool) -> Self {
        if b { OnOff::On } else { OnOff::Off }
    }
}

/// HA's `payload_reset_preset_mode` default.
pub const PRESET_NONE: &str = "None";
/// Fan presets offered to HA, named exactly like the modes in the web UI. Holiday is shown via
/// the mode sensor, not as a preset, because it needs dates.
pub const PRESETS: [&str; 3] = ["Auto", "Party", "Quiet"];

/// Faults that are notices, not problems.
fn is_problem(f: &Fault) -> bool {
    !matches!(f, Fault::FilterChange | Fault::TimeNotSet)
}

impl State {
    pub fn new(s: &Status, bypass: &BypassConfig) -> Self {
        Self {
            fan: (s.control.level > 0).into(),
            level: s.control.level,
            preset: match s.control.mode {
                Mode::Manual | Mode::Holiday => PRESET_NONE,
                Mode::Auto => PRESETS[0],
                Mode::Party => PRESETS[1],
                Mode::Quiet => PRESETS[2],
            },
            mode: s.control.mode,
            auto_return_min: s.control.auto_return_s.map(|s| s.div_ceil(60)),
            t_outdoor: s.temperatures.outdoor,
            t_supply: s.temperatures.supply,
            t_extract: s.temperatures.extract,
            t_exhaust: s.temperatures.exhaust,
            efficiency: s.efficiency_pct,
            heat_w: s.heat_recovery.power_w,
            heat_kwh: s.heat_recovery.heating_kwh,
            cool_kwh: s.heat_recovery.cooling_kwh,
            supply_rpm: s.supply_fan.rpm,
            exhaust_rpm: s.exhaust_fan.rpm,
            supply_m3h: s.supply_fan.airflow_m3h,
            exhaust_m3h: s.exhaust_fan.airflow_m3h,
            supply_v: s.supply_fan.volts,
            exhaust_v: s.exhaust_fan.volts,
            bypass: s.bypass.open.into(),
            bypass_enabled: bypass.enabled.into(),
            bypass_target: bypass.target_c,
            frost: s.frost.active.into(),
            defrost: s.frost.defrost.into(),
            frost_state: s.frost.state,
            filter_days: s.filter.remaining_days,
            filter_due: s.filter.due.into(),
            op_hours: s.operating_hours,
            problem: s.faults.iter().any(is_problem).into(),
            faults: s.faults.clone(),
        }
    }

    pub fn to_json<'b>(&self, buf: &'b mut [u8]) -> Result<&'b [u8], Error> {
        let mut o = json::Obj::new(buf);
        o.str("fan", self.fan.as_str());
        o.u32("level", self.level.into());
        o.str("preset", self.preset);
        o.any("mode", &self.mode);
        o.opt_u32("auto_return_min", self.auto_return_min);
        for (k, v) in [
            ("t_outdoor", self.t_outdoor),
            ("t_supply", self.t_supply),
            ("t_extract", self.t_extract),
            ("t_exhaust", self.t_exhaust),
            ("efficiency", self.efficiency),
            ("heat_w", self.heat_w),
            ("heat_kwh", Some(self.heat_kwh)),
            ("cool_kwh", Some(self.cool_kwh)),
        ] {
            o.f32(k, v);
        }
        o.u32("supply_rpm", self.supply_rpm);
        o.u32("exhaust_rpm", self.exhaust_rpm);
        for (k, v) in [
            ("supply_m3h", self.supply_m3h),
            ("exhaust_m3h", self.exhaust_m3h),
            ("supply_v", Some(self.supply_v)),
            ("exhaust_v", Some(self.exhaust_v)),
        ] {
            o.f32(k, v);
        }
        o.str("bypass", self.bypass.as_str());
        o.str("bypass_enabled", self.bypass_enabled.as_str());
        o.f32("bypass_target", Some(self.bypass_target));
        o.str("frost", self.frost.as_str());
        o.str("defrost", self.defrost.as_str());
        o.any("frost_state", &self.frost_state);
        o.u32("filter_days", self.filter_days);
        o.str("filter_due", self.filter_due.as_str());
        o.u32("op_hours", self.op_hours);
        o.str("problem", self.problem.as_str());
        o.any("faults", self.faults.as_slice());
        let n = o.finish()?;
        Ok(&buf[..n])
    }

    /// The serde version [`State::to_json`] replaced; the tests compare both.
    #[cfg(test)]
    fn to_json_serde<'b>(&self, buf: &'b mut [u8]) -> Result<&'b [u8], Error> {
        let n = serde_json_core::to_slice(self, buf).map_err(|_| Error::TooLong)?;
        Ok(&buf[..n])
    }
}

// ---------------------------------------------------------------------------------------
// Commands

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// `ON`: the firmware resumes the last non-zero level.
    On,
    /// `OFF`: level 0.
    Off,
    Level(u8),
    /// `Manual` ends a running program.
    Preset(Mode),
    BypassEnabled(bool),
    /// New bypass target in °C; validate via `BypassConfig::validate` before saving.
    BypassTarget(f32),
    FilterReset,
    Reboot,
}

impl Command {
    /// Parses a message received on one of the command topics.
    pub fn parse(base_topic: &str, topic: &str, payload: &[u8]) -> Result<Self, Error> {
        let sub = topic.strip_prefix(base_topic).and_then(|t| t.strip_prefix('/')).ok_or(Error::UnknownTopic)?;
        let payload = core::str::from_utf8(payload).map_err(|_| Error::BadPayload)?.trim();
        let on_off = |p: &str| match p {
            "ON" => Ok(true),
            "OFF" => Ok(false),
            _ => Err(Error::BadPayload),
        };
        let press = |p: &str, cmd| {
            if p == "PRESS" { Ok(cmd) } else { Err(Error::BadPayload) }
        };
        match sub {
            "fan/set" => on_off(payload).map(|on| if on { Command::On } else { Command::Off }),
            "fan/level/set" => match payload.parse::<u8>() {
                Ok(level) if level <= MAX_LEVEL => Ok(Command::Level(level)),
                _ => Err(Error::BadPayload),
            },
            // Case-insensitive: older automations still send the lowercase ids.
            "fan/preset/set" => match payload {
                p if p.eq_ignore_ascii_case("auto") => Ok(Command::Preset(Mode::Auto)),
                p if p.eq_ignore_ascii_case("party") => Ok(Command::Preset(Mode::Party)),
                p if p.eq_ignore_ascii_case("quiet") => Ok(Command::Preset(Mode::Quiet)),
                PRESET_NONE | "" => Ok(Command::Preset(Mode::Manual)),
                _ => Err(Error::BadPayload),
            },
            "bypass/enabled/set" => on_off(payload).map(Command::BypassEnabled),
            "bypass/target/set" => match serde_json_core::parse_float(payload).map(|f| f as f32) {
                Some(t) if t.is_finite() => Ok(Command::BypassTarget(t)),
                _ => Err(Error::BadPayload),
            },
            "filter/reset" => press(payload, Command::FilterReset),
            "system/reboot" => press(payload, Command::Reboot),
            _ => Err(Error::UnknownTopic),
        }
    }

    /// The equivalent `PATCH /control` request for fan commands, `None` for settings
    /// commands. `last_level` is the most recent non-zero level (used by `ON`). Party/quiet
    /// use the stored program defaults.
    pub fn to_request(self, last_level: u8) -> Option<ControlRequest> {
        let level = |l| ControlRequest { level: Some(l), ..Default::default() };
        let mode = |m| ControlRequest { mode: Some(m), ..Default::default() };
        Some(match self {
            Command::On => level(last_level.clamp(1, MAX_LEVEL)),
            Command::Off => level(0),
            Command::Level(l) => level(l),
            Command::Preset(m) => mode(m),
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------------------
// Discovery

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Fan,
    Sensor,
    BinarySensor,
    Switch,
    Number,
    Button,
}

impl Component {
    pub const fn as_str(self) -> &'static str {
        match self {
            Component::Fan => "fan",
            Component::Sensor => "sensor",
            Component::BinarySensor => "binary_sensor",
            Component::Switch => "switch",
            Component::Number => "number",
            Component::Button => "button",
        }
    }

    /// Buttons have no state.
    const fn has_state(self) -> bool {
        !matches!(self, Component::Button)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Primary,
    Config,
    Diagnostic,
}

/// `number` range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

/// Static part of one HA entity.
#[derive(Debug, Clone, Copy)]
pub struct Entity {
    pub component: Component,
    pub object_id: &'static str,
    /// `None` uses the device name (HA convention for the main entity).
    pub name: Option<&'static str>,
    pub value_template: Option<&'static str>,
    pub command_topic: Option<&'static str>,
    pub device_class: Option<&'static str>,
    pub unit: Option<&'static str>,
    pub state_class: Option<&'static str>,
    pub category: Category,
    pub icon: Option<&'static str>,
    pub options: Option<&'static [&'static str]>,
    pub attributes_template: Option<&'static str>,
    pub range: Option<Range>,
}

const BASE: Entity = Entity {
    component: Component::Sensor,
    object_id: "",
    name: None,
    value_template: None,
    command_topic: None,
    device_class: None,
    unit: None,
    state_class: None,
    category: Category::Primary,
    icon: None,
    options: None,
    attributes_template: None,
    range: None,
};

const fn sensor(object_id: &'static str, name: &'static str, template: &'static str) -> Entity {
    Entity { object_id, name: Some(name), value_template: Some(template), state_class: Some("measurement"), ..BASE }
}

const fn temperature(object_id: &'static str, name: &'static str, template: &'static str) -> Entity {
    Entity { device_class: Some("temperature"), unit: Some("°C"), ..sensor(object_id, name, template) }
}

const fn diagnostic(e: Entity) -> Entity {
    Entity { category: Category::Diagnostic, ..e }
}

const fn binary(object_id: &'static str, name: &'static str, template: &'static str, class: &'static str) -> Entity {
    Entity {
        component: Component::BinarySensor,
        object_id,
        name: Some(name),
        value_template: Some(template),
        device_class: Some(class),
        ..BASE
    }
}

const fn button(object_id: &'static str, name: &'static str, topic: &'static str) -> Entity {
    Entity { component: Component::Button, object_id, name: Some(name), command_topic: Some(topic), ..BASE }
}

pub const FROST_STATES: &[&str] =
    &["idle", "armed", "preheat_wait", "boost_start", "boost_check", "boost_hold", "defrost_check", "defrost", "test_run"];
/// Mode sensor states, named exactly like the modes in the web UI.
pub const MODES: &[&str] = &["Manual", "Auto", "Party", "Quiet", "Holiday"];

pub const ENTITIES: &[Entity] = &[
    // Main control.
    Entity { component: Component::Fan, object_id: "fan", icon: Some("mdi:hvac"), command_topic: Some("~/fan/set"), ..BASE },
    Entity {
        object_id: "mode",
        name: Some("Mode"),
        value_template: Some("{{ value_json.mode | capitalize }}"),
        device_class: Some("enum"),
        options: Some(MODES),
        icon: Some("mdi:calendar-clock"),
        ..BASE
    },
    Entity {
        device_class: Some("duration"),
        unit: Some("min"),
        state_class: None,
        icon: Some("mdi:timer-sand"),
        ..sensor("auto_return", "Returns to Auto in", "{{ value_json.auto_return_min }}")
    },
    // Air temperatures and heat recovery.
    temperature("t_outdoor", "Outdoor air", "{{ value_json.t_outdoor }}"),
    temperature("t_supply", "Supply air", "{{ value_json.t_supply }}"),
    temperature("t_extract", "Extract air", "{{ value_json.t_extract }}"),
    temperature("t_exhaust", "Exhaust air", "{{ value_json.t_exhaust }}"),
    Entity {
        unit: Some("%"),
        icon: Some("mdi:heat-wave"),
        ..sensor(
            "efficiency",
            "Heat recovery efficiency",
            "{{ value_json.efficiency | round(1) if value_json.efficiency is not none else None }}",
        )
    },
    Entity {
        device_class: Some("volume_flow_rate"),
        unit: Some("m³/h"),
        ..sensor(
            "supply_airflow",
            "Supply air flow",
            "{{ value_json.supply_m3h | round(0) if value_json.supply_m3h is not none else None }}",
        )
    },
    Entity {
        device_class: Some("volume_flow_rate"),
        unit: Some("m³/h"),
        ..sensor(
            "exhaust_airflow",
            "Extract air flow",
            "{{ value_json.exhaust_m3h | round(0) if value_json.exhaust_m3h is not none else None }}",
        )
    },
    Entity {
        device_class: Some("power"),
        unit: Some("W"),
        ..sensor(
            "heat_power",
            "Recovered heat power",
            "{{ value_json.heat_w | round(0) if value_json.heat_w is not none else None }}",
        )
    },
    Entity {
        device_class: Some("energy"),
        unit: Some("kWh"),
        state_class: Some("total_increasing"),
        ..sensor("heat_energy", "Recovered heat", "{{ value_json.heat_kwh | round(2) }}")
    },
    Entity {
        device_class: Some("energy"),
        unit: Some("kWh"),
        state_class: Some("total_increasing"),
        ..sensor("cool_energy", "Recovered cooling", "{{ value_json.cool_kwh | round(2) }}")
    },
    // Filter.
    Entity {
        device_class: Some("duration"),
        unit: Some("d"),
        state_class: None,
        icon: Some("mdi:air-filter"),
        ..sensor("filter_remaining", "Filter change in", "{{ value_json.filter_days }}")
    },
    binary("filter_due", "Filter change due", "{{ value_json.filter_due }}", "problem"),
    Entity {
        category: Category::Config,
        icon: Some("mdi:air-filter"),
        ..button("filter_reset", "Reset filter interval", "~/filter/reset")
    },
    // Bypass.
    binary("bypass", "Summer bypass", "{{ value_json.bypass }}", "opening"),
    Entity {
        component: Component::Switch,
        object_id: "bypass_enabled",
        name: Some("Bypass cooling"),
        value_template: Some("{{ value_json.bypass_enabled }}"),
        command_topic: Some("~/bypass/enabled/set"),
        category: Category::Config,
        icon: Some("mdi:valve"),
        ..BASE
    },
    Entity {
        component: Component::Number,
        object_id: "bypass_target",
        name: Some("Bypass target temperature"),
        value_template: Some("{{ value_json.bypass_target }}"),
        command_topic: Some("~/bypass/target/set"),
        device_class: Some("temperature"),
        unit: Some("°C"),
        category: Category::Config,
        range: Some(Range { min: 10.0, max: 35.0, step: 0.5 }),
        ..BASE
    },
    // Frost protection and faults.
    binary("frost", "Frost protection", "{{ value_json.frost }}", "cold"),
    binary("defrost", "Defrosting", "{{ value_json.defrost }}", "running"),
    Entity {
        attributes_template: Some("{{ {'faults': value_json.faults} | tojson }}"),
        ..binary("problem", "Fault", "{{ value_json.problem }}", "problem")
    },
    // Diagnostics.
    diagnostic(Entity {
        device_class: Some("enum"),
        state_class: None,
        options: Some(FROST_STATES),
        icon: Some("mdi:snowflake-thermometer"),
        ..sensor("frost_state", "Frost protection state", "{{ value_json.frost_state }}")
    }),
    diagnostic(Entity {
        unit: Some("rpm"),
        icon: Some("mdi:fan"),
        ..sensor("supply_rpm", "Supply fan speed", "{{ value_json.supply_rpm }}")
    }),
    diagnostic(Entity {
        unit: Some("rpm"),
        icon: Some("mdi:fan"),
        ..sensor("exhaust_rpm", "Exhaust fan speed", "{{ value_json.exhaust_rpm }}")
    }),
    diagnostic(Entity {
        device_class: Some("voltage"),
        unit: Some("V"),
        ..sensor("supply_v", "Supply fan voltage", "{{ value_json.supply_v }}")
    }),
    diagnostic(Entity {
        device_class: Some("voltage"),
        unit: Some("V"),
        ..sensor("exhaust_v", "Exhaust fan voltage", "{{ value_json.exhaust_v }}")
    }),
    diagnostic(Entity {
        device_class: Some("duration"),
        unit: Some("h"),
        state_class: Some("total_increasing"),
        ..sensor("op_hours", "Operating hours", "{{ value_json.op_hours }}")
    }),
    diagnostic(Entity { device_class: Some("restart"), ..button("reboot", "Restart", "~/system/reboot") }),
];

#[cfg(test)]
#[derive(Serialize)]
struct DeviceInfo<'a> {
    #[serde(rename = "ids")]
    identifiers: [&'a str; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cns")]
    connections: Option<[[&'a str; 2]; 1]>,
    name: &'a str,
    #[serde(rename = "mf")]
    manufacturer: &'static str,
    #[serde(rename = "mdl")]
    model: &'a str,
    #[serde(rename = "sw")]
    sw_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cu")]
    configuration_url: Option<&'a str>,
}

#[cfg(test)]
#[derive(Serialize)]
struct Origin<'a> {
    name: &'static str,
    #[serde(rename = "sw")]
    sw_version: &'a str,
}

/// Discovery config with Home Assistant's documented key abbreviations (`stat_t`, `cmd_t`, …):
/// shorter messages and less flash for the key strings.
#[cfg(test)]
#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "~")]
    base: &'a str,
    /// `null` (not left out) for the main entity: HA then names it after the device instead of
    /// its default "MQTT Fan".
    name: Option<&'a str>,
    #[serde(rename = "uniq_id")]
    unique_id: &'a str,
    #[serde(rename = "avty_t")]
    availability_topic: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stat_t")]
    state_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "val_tpl")]
    value_template: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cmd_t")]
    command_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "dev_cla")]
    device_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "unit_of_meas")]
    unit_of_measurement: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stat_cla")]
    state_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ent_cat")]
    entity_category: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ic")]
    icon: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ops")]
    options: Option<&'static [&'static str]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "json_attr_t")]
    json_attributes_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "json_attr_tpl")]
    json_attributes_template: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    // Fan-only keys (`serde(flatten)` needs `alloc`, so they are listed here). Levels map
    // to HA percentages via `speed_range` 1–4, so HA sends and expects the level itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "stat_val_tpl")]
    state_value_template: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pct_cmd_t")]
    percentage_command_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pct_stat_t")]
    percentage_state_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pct_val_tpl")]
    percentage_value_template: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "spd_rng_min")]
    speed_range_min: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "spd_rng_max")]
    speed_range_max: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pr_mode_cmd_t")]
    preset_mode_command_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pr_mode_stat_t")]
    preset_mode_state_topic: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pr_mode_val_tpl")]
    preset_mode_value_template: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "pr_modes")]
    preset_modes: Option<[&'static str; 3]>,
    #[serde(rename = "dev")]
    device: DeviceInfo<'a>,
    #[serde(rename = "o")]
    origin: Origin<'a>,
}

impl Device<'_> {
    /// Unique id of an entity: `<node_id>_<object_id>`.
    pub fn unique_id(&self, e: &Entity) -> Result<heapless::String<64>, Error> {
        let mut id = heapless::String::new();
        write!(id, "{}_{}", self.node_id, e.object_id).map_err(|_| Error::TooLong)?;
        Ok(id)
    }

    /// Writes the retained discovery payload for `e` into `buf`: HA's documented key
    /// abbreviations (`stat_t`, `cmd_t`, …), `~` for the base topic.
    pub fn discovery_payload<'b>(&self, e: &Entity, buf: &'b mut [u8]) -> Result<&'b [u8], Error> {
        let unique_id = self.unique_id(e)?;
        let fan = e.component == Component::Fan;
        let if_fan = |v: &'static str| fan.then_some(v);
        let mut o = json::Obj::new(buf);
        o.str("~", self.base_topic);
        // `null` (not left out) for the main entity: HA then names it after the device
        // instead of its default "MQTT Fan".
        o.str_or_null("name", e.name);
        o.str("uniq_id", &unique_id);
        o.str("avty_t", "~/availability");
        for (k, v) in [
            ("stat_t", e.component.has_state().then_some("~/state")),
            ("val_tpl", e.value_template),
            ("cmd_t", e.command_topic),
            ("dev_cla", e.device_class),
            ("unit_of_meas", e.unit),
            ("stat_cla", e.state_class),
            (
                "ent_cat",
                match e.category {
                    Category::Primary => None,
                    Category::Config => Some("config"),
                    Category::Diagnostic => Some("diagnostic"),
                },
            ),
            ("ic", e.icon),
        ] {
            o.opt_str(k, v);
        }
        if let Some(ops) = e.options {
            o.strs("ops", ops);
        }
        o.opt_str("json_attr_t", e.attributes_template.map(|_| "~/state"));
        o.opt_str("json_attr_tpl", e.attributes_template);
        if let Some(r) = e.range {
            o.f32("min", Some(r.min));
            o.f32("max", Some(r.max));
            o.f32("step", Some(r.step));
            o.str("mode", "box");
        }
        // Fan-only keys. Levels map to HA percentages via the speed range 1–4, so HA sends
        // and expects the level itself.
        for (k, v) in [
            ("stat_val_tpl", if_fan("{{ value_json.fan }}")),
            ("pct_cmd_t", if_fan("~/fan/level/set")),
            ("pct_stat_t", if_fan("~/state")),
            ("pct_val_tpl", if_fan("{{ value_json.level }}")),
        ] {
            o.opt_str(k, v);
        }
        if fan {
            o.u32("spd_rng_min", 1);
            o.u32("spd_rng_max", MAX_LEVEL.into());
        }
        for (k, v) in [
            ("pr_mode_cmd_t", if_fan("~/fan/preset/set")),
            ("pr_mode_stat_t", if_fan("~/state")),
            ("pr_mode_val_tpl", if_fan("{{ value_json.preset }}")),
        ] {
            o.opt_str(k, v);
        }
        if fan {
            o.strs("pr_modes", &PRESETS);
        }
        o.open("dev");
        o.strs("ids", &[self.node_id]);
        if !self.mac.is_empty() {
            o.open_list("cns");
            o.strs_item(&["mac", self.mac]);
            o.close_list();
        }
        o.str("name", self.name);
        o.str("mf", "Helios");
        o.str("mdl", self.model_or_unknown());
        o.str("sw", self.sw_version);
        o.opt_str("cu", self.configuration_url);
        o.close();
        o.open("o");
        o.str("name", "openKWL");
        o.str("sw", self.sw_version);
        o.close();
        let n = o.finish()?;
        Ok(&buf[..n])
    }

    /// The serde version [`Device::discovery_payload`] replaced; the tests compare both.
    #[cfg(test)]
    fn discovery_payload_serde<'b>(&self, e: &Entity, buf: &'b mut [u8]) -> Result<&'b [u8], Error> {
        let unique_id = self.unique_id(e)?;
        let fan = e.component == Component::Fan;
        let if_fan = |v: &'static str| fan.then_some(v);
        let payload = Payload {
            base: self.base_topic,
            name: e.name,
            unique_id: &unique_id,
            availability_topic: "~/availability",
            state_topic: e.component.has_state().then_some("~/state"),
            value_template: e.value_template,
            command_topic: e.command_topic,
            device_class: e.device_class,
            unit_of_measurement: e.unit,
            state_class: e.state_class,
            entity_category: match e.category {
                Category::Primary => None,
                Category::Config => Some("config"),
                Category::Diagnostic => Some("diagnostic"),
            },
            icon: e.icon,
            options: e.options,
            json_attributes_topic: e.attributes_template.map(|_| "~/state"),
            json_attributes_template: e.attributes_template,
            min: e.range.map(|r| r.min),
            max: e.range.map(|r| r.max),
            step: e.range.map(|r| r.step),
            mode: e.range.map(|_| "box"),
            state_value_template: if_fan("{{ value_json.fan }}"),
            percentage_command_topic: if_fan("~/fan/level/set"),
            percentage_state_topic: if_fan("~/state"),
            percentage_value_template: if_fan("{{ value_json.level }}"),
            speed_range_min: fan.then_some(1),
            speed_range_max: fan.then_some(MAX_LEVEL),
            preset_mode_command_topic: if_fan("~/fan/preset/set"),
            preset_mode_state_topic: if_fan("~/state"),
            preset_mode_value_template: if_fan("{{ value_json.preset }}"),
            preset_modes: fan.then_some(PRESETS),
            device: DeviceInfo {
                identifiers: [self.node_id],
                connections: (!self.mac.is_empty()).then_some([["mac", self.mac]]),
                name: self.name,
                manufacturer: "Helios",
                model: self.model_or_unknown(),
                sw_version: self.sw_version,
                configuration_url: self.configuration_url,
            },
            origin: Origin { name: "openKWL", sw_version: self.sw_version },
        };
        let n = serde_json_core::to_slice(&payload, buf).map_err(|_| Error::TooLong)?;
        Ok(&buf[..n])
    }
}

#[cfg(test)]
mod tests;
