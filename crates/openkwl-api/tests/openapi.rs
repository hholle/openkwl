//! Keeps `api/openapi.yaml` in sync with the Rust types: every schema's property names and
//! required list must match the serialised Rust struct, and every string enum must list
//! exactly the Rust variants.

use openkwl_api::*;
use openkwl_core::clock;

fn md(month: u8, day: u8) -> time::MonthDay {
    time::MonthDay(clock::MonthDay::new(month, day).unwrap())
}
use serde::Serialize;
use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

fn schemas() -> Yaml {
    let text = include_str!("../../../api/openapi.yaml");
    let doc = YamlLoader::load_from_str(text).expect("valid YAML").remove(0);
    doc["components"]["schemas"].clone()
}

fn strings(y: &Yaml) -> Vec<String> {
    let mut v: Vec<String> = y.as_vec().map(|a| a.iter().map(|s| s.as_str().unwrap().to_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// `value` must have exactly the schema's properties. Optional fields that Rust skips when
/// `None` are listed in `skippable` and must not be `required` in the schema.
fn assert_object(schemas: &Yaml, name: &str, value: &impl Serialize, skippable: &[&str]) {
    let schema = &schemas[name];
    assert!(!schema.is_badvalue(), "schema {name} missing");
    let Value::Object(obj) = serde_json::to_value(value).unwrap() else { panic!("{name} is not an object") };

    let mut props: Vec<String> = schema["properties"].as_hash().unwrap().keys().map(|k| k.as_str().unwrap().to_owned()).collect();
    props.sort();
    let mut keys: Vec<String> = obj.keys().cloned().collect();
    keys.extend(skippable.iter().map(|s| (*s).to_owned()).filter(|s| !obj.contains_key(s)));
    keys.sort();
    assert_eq!(props, keys, "properties of {name}");

    let mut required: Vec<String> = obj.keys().filter(|k| !skippable.contains(&k.as_str())).cloned().collect();
    required.sort();
    assert_eq!(strings(&schema["required"]), required, "required fields of {name}");
}

fn assert_enum<T: Serialize>(schemas: &Yaml, name: &str, variants: &[T]) {
    let mut names: Vec<String> = variants.iter().map(|v| serde_json::to_value(v).unwrap().as_str().unwrap().to_owned()).collect();
    names.sort();
    assert_eq!(strings(&schemas[name]["enum"]), names, "enum {name}");
}

fn s<const N: usize>(v: &str) -> heapless::String<N> {
    v.try_into().unwrap()
}

#[test]
fn enums_match() {
    let y = schemas();
    assert_enum(&y, "Mode", &[Mode::Manual, Mode::Auto, Mode::Party, Mode::Quiet, Mode::Holiday]);
    assert_enum(&y, "BaseMode", &[BaseMode::Manual, BaseMode::Auto]);
    assert_enum(
        &y,
        "FrostState",
        &[
            FrostState::Idle,
            FrostState::Armed,
            FrostState::PreheatWait,
            FrostState::BoostStart,
            FrostState::BoostCheck,
            FrostState::BoostHold,
            FrostState::DefrostCheck,
            FrostState::Defrost,
            FrostState::TestRun,
        ],
    );
    assert_enum(
        &y,
        "Fault",
        &[
            Fault::SupplyFanStall,
            Fault::ExhaustFanStall,
            Fault::OutdoorSensorOpen,
            Fault::SupplySensorOpen,
            Fault::ExtractSensorOpen,
            Fault::ExhaustSensorOpen,
            Fault::OutdoorSensorShort,
            Fault::SupplySensorShort,
            Fault::ExtractSensorShort,
            Fault::ExhaustSensorShort,
            Fault::SdCard,
            Fault::FilterChange,
            Fault::TimeNotSet,
            Fault::ConfigInvalid,
            Fault::BusOvercurrent,
        ],
    );
    assert_enum(&y, "ActuatorType", &[ActuatorType::Analog, ActuatorType::Motor, ActuatorType::MotorReversed]);
    assert_enum(
        &y,
        "ResetReason",
        &[
            ResetReason::PowerOn,
            ResetReason::Pin,
            ResetReason::Software,
            ResetReason::Watchdog,
            ResetReason::Brownout,
            ResetReason::Unknown,
        ],
    );
    assert_enum(&y, "TimeSource", &[TimeSource::None, TimeSource::Rtc, TimeSource::Manual, TimeSource::Sntp]);
    use ErrorCode::*;
    let codes = [
        BadRequest,
        Invalid,
        Unauthorized,
        SetupRequired,
        NotFound,
        MethodNotAllowed,
        Conflict,
        PayloadTooLarge,
        TooManyRequests,
        Internal,
        Unavailable,
    ];
    let mut names: Vec<String> = codes.iter().map(|c| serde_json::to_value(c).unwrap().as_str().unwrap().to_owned()).collect();
    names.sort();
    assert_eq!(strings(&y["ApiError"]["properties"]["error"]["enum"]), names);
}

#[test]
fn objects_match() {
    let y = schemas();
    let control =
        Control { mode: Mode::Party, level: 3, remaining_s: Some(60), auto_return_s: None, base: BaseMode::Manual, manual_level: 2 };
    let fan = Fan { volts: 4.0, duty_pct: 40, rpm: 1200, stalled: false, airflow_m3h: Some(150.0) };
    let temps = Temperatures { outdoor: Some(1.0), supply: Some(2.0), extract: Some(3.0), exhaust: None };
    let bypass = BypassStatus { open: true, moving: false };
    let frost = FrostStatus::from(openkwl_core::frost::FrostController::new(openkwl_core::frost::FrostParams::STOCK).outputs());

    assert_object(&y, "Control", &control, &[]);
    assert_object(&y, "ControlRequest", &ControlRequest::default(), &["mode", "level", "duration_min", "end_program"]);
    assert_object(&y, "Fan", &fan, &[]);
    assert_object(&y, "Temperatures", &temps, &[]);
    assert_object(&y, "BypassStatus", &bypass, &[]);
    assert_object(&y, "FrostStatus", &frost, &[]);
    assert_object(
        &y,
        "Status",
        &Status {
            uptime_s: 1,
            time: None,
            control,
            schedule: None,
            holiday: None,
            temperatures: temps,
            supply_fan: fan,
            exhaust_fan: fan,
            efficiency_pct: None,
            bypass,
            frost,
            heat_recovery: HeatRecovery { power_w: None, heating_kwh: 0.0, cooling_kwh: 0.0 },
            filter: FilterStatus { remaining_days: 1, due: false },
            operating_hours: 1,
            faults: heapless::Vec::new(),
        },
        &[],
    );

    assert_object(&y, "LoginRequest", &LoginRequest { password: s("pw"), remember: true }, &["remember"]);
    assert_object(&y, "LoginResponse", &LoginResponse { token: s("ab"), idle_timeout_s: 1, expires_in_s: 2 }, &[]);
    assert_object(&y, "PasswordChangeRequest", &PasswordChangeRequest { current_password: s("a"), new_password: s("b") }, &[]);
    assert_object(&y, "ApiError", &ApiError::new(ErrorCode::Internal, "x"), &["field", "retry_after_s"]);

    let bypass_cfg = BypassConfig {
        enabled: true,
        target_c: 23.0,
        min_outdoor_c: 15.0,
        close_if_outdoor_warmer: true,
        actuator: ActuatorType::Motor,
        run_time_s: 120,
        season: Some(Season { start: md(3, 1), end: md(10, 2) }),
        supply_offset_k: 4.0,
    };
    assert_object(&y, "BypassConfig", &bypass_cfg, &[]);
    assert_object(&y, "Season", &bypass_cfg.season.unwrap(), &[]);
    assert_object(&y, "HeatRecovery", &HeatRecovery { power_w: None, heating_kwh: 0.0, cooling_kwh: 0.0 }, &[]);
    assert_object(&y, "FilterStatus", &FilterStatus { remaining_days: 1, due: false }, &[]);
    assert_object(&y, "FanConfig", &FanConfig::STOCK, &[]);
    assert_object(&y, "ProgramConfig", &ProgramConfig { party_level: 4, party_min: 150, quiet_level: 0, quiet_min: 180 }, &[]);
    let date = |d| time::Date(clock::Date::new(2024, 7, d).unwrap());
    assert_object(
        &y,
        "HolidayConfig",
        &HolidayConfig { enabled: true, start: date(13), end: date(19), level: 1, interval_h: 2, active_min: 60 },
        &[],
    );
    assert_object(&y, "TimeConfig", &TimeConfig::default_cet(), &[]);
    assert_object(&y, "FilterConfig", &FilterConfig { interval_months: 3 }, &[]);
    assert_object(
        &y,
        "ScheduleConfig",
        &ScheduleConfig { active_profile: 1, auto_at_midnight: true, auto_return_min: Some(120), fallback_level: 2 },
        &[],
    );
    let slot = Slot { from: time::TimeOfDay::new(6, 30).unwrap(), level: 3 };
    assert_object(&y, "Slot", &slot, &[]);
    let next = NextChange { at: slot.from, level: 3, tomorrow: false };
    assert_object(&y, "NextChange", &next, &[]);
    let today = ScheduleToday { profile: 1, today: heapless::Vec::from_slice(&[slot]).unwrap(), next: Some(next) };
    assert_object(&y, "ScheduleToday", &today, &[]);
    assert_object(&y, "HolidayStatus", &HolidayStatus { until: date(19), burst: true }, &[]);
    assert_object(&y, "ScheduleProfile", &ScheduleProfile { days: Default::default() }, &[]);
    assert_object(&y, "SetupStatus", &SetupStatus { setup_required: true, window_remaining_s: None }, &[]);
    assert_object(&y, "SetupRequest", &SetupRequest { password: s("x") }, &[]);
    assert_object(&y, "SetTimeRequest", &SetTimeRequest { unix_s: 1 }, &[]);
    assert_object(&y, "FrostConfig", &FrostConfig::from(openkwl_core::frost::FrostParams::STOCK), &[]);
    assert_object(
        &y,
        "NetworkConfig",
        &NetworkConfig { dhcp: true, hostname: s("kwl"), address: None, netmask: None, gateway: None, dns: None },
        &[],
    );
    let update = MqttConfigUpdate {
        enabled: true,
        host: s("h"),
        port: 1883,
        username: s(""),
        password: None,
        client_id: s("c"),
        base_topic: s("b"),
        discovery: true,
        discovery_prefix: s("homeassistant"),
    };
    assert_object(&y, "MqttConfigUpdate", &update, &["password"]);
    assert_object(&y, "MqttConfig", &update.applied(false), &[]);
    assert_object(
        &y,
        "DeviceIdentity",
        &DeviceIdentity {
            model: s("KWL EC 300W R"),
            article: s("04232001.hGP"),
            serial: s("1000"),
            production_code: s("01001"),
            security_code: s(""),
        },
        &[],
    );
    assert_object(
        &y,
        "SystemInfo",
        &SystemInfo {
            firmware_version: s("0.1.0"),
            build: s("dev"),
            mcu: s("STM32F207"),
            mac: s("00:00:00:00:00:00"),
            ip: None,
            uptime_s: 0,
            reset_reason: ResetReason::PowerOn,
            sd_card: true,
            mqtt_connected: false,
            last_time_sync: None,
            device: DeviceIdentity {
                model: s("KWL EC 300W R"),
                article: s("04232001.hGP"),
                serial: s("1000"),
                production_code: s("01001"),
                security_code: s(""),
            },
        },
        &[],
    );

    let d = SystemDetails::default();
    assert_object(&y, "SystemDetails", &d, &[]);
    assert_object(&y, "FirmwareDetails", &d.firmware, &[]);
    assert_object(&y, "ChipDetails", &d.chip, &[]);
    assert_object(&y, "MemoryDetails", &d.memory, &[]);
    assert_object(&y, "NetworkDetails", &d.network, &[]);
    assert_object(&y, "TimeDetails", &d.time, &[]);
    assert_object(&y, "MqttDetails", &d.mqtt, &[]);
    assert_object(&y, "StorageDetails", &d.storage, &[]);
    assert_object(&y, "StockDetails", &d.stock, &[]);
    assert_object(&y, "Counters", &d.counters, &[]);
    assert_object(&y, "BoardDetails", &d.board, &[]);
    assert_object(&y, "ModuleDetails", &d.modules, &[]);
    let enum_of = |v: serde_json::Value| v.as_str().unwrap().to_owned();
    let mut power: Vec<String> = [BusPower::Off, BusPower::Starting, BusPower::On, BusPower::Overcurrent]
        .map(|p| enum_of(serde_json::to_value(p).unwrap()))
        .to_vec();
    power.sort();
    assert_eq!(strings(&y["ModuleDetails"]["properties"]["bus_power"]["enum"]), power);
    let mut knx: Vec<String> = [KnxState::Searching, KnxState::Connected, KnxState::NotFound]
        .map(|k| enum_of(serde_json::to_value(k).unwrap()))
        .to_vec();
    knx.sort();
    assert_eq!(strings(&y["ModuleDetails"]["properties"]["knx"]["enum"]), knx);
}

/// The `minimum`/`maximum` of every `FrostConfig` property are exactly what `validate` accepts.
#[test]
fn frost_limits_match() {
    let y = schemas();
    let stock = serde_json::to_value(FrostConfig::from(openkwl_core::frost::FrostParams::STOCK)).unwrap();
    let props = y["FrostConfig"]["properties"].as_hash().unwrap();
    for (name, p) in props {
        let name = name.as_str().unwrap();
        let num = |k: &str| p[k].as_f64().or_else(|| p[k].as_i64().map(|i| i as f64)).unwrap();
        let step = if p["type"].as_str() == Some("integer") { 1.0 } else { 0.01 };
        let with = |v: f64| {
            let mut j = stock.clone();
            j[name] = if step == 1.0 { serde_json::json!(v as i64) } else { serde_json::json!(v) };
            // Keep the cross-field rule `entry_pct <= hold_pct` out of the way.
            match name {
                "entry_pct" => j["hold_pct"] = serde_json::json!(100.0),
                "hold_pct" => j["entry_pct"] = serde_json::json!(0.0),
                _ => {}
            }
            serde_json::from_value::<FrostConfig>(j).unwrap().validate()
        };
        let (min, max) = (num("minimum"), num("maximum"));
        assert!(with(min).is_ok(), "{name} = {min}");
        assert!(with(max).is_ok(), "{name} = {max}");
        assert_eq!(with(min - step).unwrap_err().field, name, "{name} below minimum");
        assert_eq!(with(max + step).unwrap_err().field, name, "{name} above maximum");
    }
}
