extern crate std;

use std::{collections::HashSet, string::String, vec::Vec};

use openkwl_api::{
    ActuatorType, BaseMode, BypassStatus, Control, Fan, FilterStatus, FrostStatus, HeatRecovery, Temperatures, Validate,
};
use serde_json::Value;

use super::*;

const DEV: Device<'static> = Device {
    node_id: "openkwl",
    name: "KWL",
    sw_version: "0.1.0",
    model: "KWL EC 300W R",
    mac: "80:4b:20:12:34:56",
    configuration_url: Some("http://192.168.1.50/"),
    base_topic: "openkwl",
    discovery_prefix: "homeassistant",
};

fn payload(dev: &Device, e: &Entity) -> Value {
    let mut buf = [0u8; DISCOVERY_JSON_MAX];
    let json = dev.discovery_payload(e, &mut buf).expect("fits");
    serde_json::from_slice(json).expect("valid JSON")
}

fn entity(id: &str) -> &'static Entity {
    ENTITIES.iter().find(|e| e.object_id == id).unwrap()
}

/// Expands HA's `~` abbreviation like Home Assistant does (start or end of a topic value).
fn expand(v: &Value, base: &str, key: &str) -> Option<String> {
    let t = v[key].as_str()?;
    Some(if let Some(rest) = t.strip_prefix('~') {
        std::format!("{base}{rest}")
    } else if let Some(rest) = t.strip_suffix('~') {
        std::format!("{rest}{base}")
    } else {
        t.into()
    })
}

fn bypass() -> BypassConfig {
    BypassConfig {
        enabled: true,
        target_c: 22.0,
        min_outdoor_c: 7.0,
        close_if_outdoor_warmer: true,
        actuator: ActuatorType::Motor,
        run_time_s: 120,
        season: None,
        supply_offset_k: 4.0,
    }
}

fn status() -> Status {
    Status {
        uptime_s: 10,
        time: None,
        control: Control {
            mode: Mode::Party,
            level: 4,
            remaining_s: Some(600),
            auto_return_s: Some(61),
            base: BaseMode::Manual,
            manual_level: 2,
        },
        schedule: None,
        holiday: None,
        temperatures: Temperatures { outdoor: Some(-8.5), supply: Some(15.0), extract: Some(21.0), exhaust: None },
        supply_fan: Fan { volts: 9.0, duty_pct: 90, rpm: 2400, stalled: false, airflow_m3h: Some(150.0) },
        exhaust_fan: Fan { volts: 9.0, duty_pct: 90, rpm: 2500, stalled: false, airflow_m3h: Some(150.0) },
        efficiency_pct: Some(78.46),
        bypass: BypassStatus { open: false, moving: false },
        frost: FrostStatus { state: FrostState::Idle, active: false, supply_stopped: false, exhaust_boost_v: 0.0, defrost: false },
        heat_recovery: HeatRecovery { power_w: Some(351.8), heating_kwh: 1234.5, cooling_kwh: 3.25 },
        filter: FilterStatus { remaining_days: 50, due: false },
        operating_hours: 103_896,
        faults: heapless::Vec::from_slice(&[Fault::ExhaustSensorOpen]).unwrap(),
    }
}

#[test]
fn topics() {
    assert_eq!(DEV.availability_topic().unwrap(), "openkwl/availability");
    assert_eq!(DEV.state_topic().unwrap(), "openkwl/state");
    assert_eq!(DEV.ha_status_topic().unwrap(), "homeassistant/status");
    let filters = DEV.command_filters().unwrap();
    assert_eq!(
        filters.map(|t| String::from(t.as_str())),
        ["openkwl/fan/#", "openkwl/bypass/#", "openkwl/filter/#", "openkwl/system/#"]
    );
    assert_eq!(DEV.discovery_topic(entity("fan")).unwrap(), "homeassistant/fan/openkwl/fan/config");
    assert_eq!(DEV.discovery_topic(entity("bypass")).unwrap(), "homeassistant/binary_sensor/openkwl/bypass/config");
    assert_eq!(DEV.discovery_topic(entity("reboot")).unwrap(), "homeassistant/button/openkwl/reboot/config");
}

#[test]
fn every_entity_has_valid_discovery() {
    let mut ids = HashSet::new();
    for e in ENTITIES {
        let v = payload(&DEV, e);
        assert_eq!(v["~"], "openkwl");
        assert_eq!(expand(&v, "openkwl", "avty_t").unwrap(), "openkwl/availability");
        let device = &v["dev"];
        assert_eq!(device["ids"][0], "openkwl");
        assert_eq!(device["cns"][0], serde_json::json!(["mac", "80:4b:20:12:34:56"]));
        assert_eq!(device["cu"], "http://192.168.1.50/");
        assert_eq!(v["o"]["name"], "openKWL");
        let uid = v["uniq_id"].as_str().unwrap();
        assert_eq!(uid, std::format!("openkwl_{}", e.object_id));
        assert!(ids.insert(String::from(uid)), "duplicate unique_id {uid}");

        match e.component {
            Component::Button => assert!(v.get("stat_t").is_none(), "buttons are stateless"),
            Component::Fan => assert!(v.get("value_template").is_none()),
            _ => {
                assert_eq!(expand(&v, "openkwl", "stat_t").unwrap(), "openkwl/state");
                assert!(v["val_tpl"].as_str().unwrap().contains("value_json."), "{}", e.object_id);
            }
        }
        let controllable = matches!(e.component, Component::Fan | Component::Switch | Component::Number | Component::Button);
        assert_eq!(v.get("cmd_t").is_some(), controllable, "{}", e.object_id);
        assert_eq!(v.get("pr_modes").is_some(), e.component == Component::Fan);
        if e.device_class == Some("enum") {
            assert!(v["ops"].is_array());
        }
        if e.state_class == Some("total_increasing") {
            assert!(matches!(e.device_class, Some("energy" | "duration")), "{}", e.object_id);
        }
    }
}

#[test]
fn every_command_topic_parses() {
    for e in ENTITIES {
        let v = payload(&DEV, e);
        for (key, payload) in [("cmd_t", "ON"), ("pct_cmd_t", "3"), ("pr_mode_cmd_t", "auto")] {
            let Some(topic) = expand(&v, DEV.base_topic, key) else {
                continue;
            };
            let payload = match e.component {
                Component::Button => "PRESS",
                Component::Number => "22.5",
                _ => payload,
            };
            assert!(Command::parse(DEV.base_topic, &topic, payload.as_bytes()).is_ok(), "{topic} {payload}");
        }
    }
}

#[test]
fn fan_entity() {
    let v = payload(&DEV, entity("fan"));
    assert!(v["name"].is_null() && v.get("name").is_some(), "explicit null: the main entity takes the device name");
    // Speed and preset follow the unit (web UI, schedule), not only what HA sent last.
    assert_eq!((v["pct_stat_t"].as_str(), v["pr_mode_stat_t"].as_str()), (Some("~/state"), Some("~/state")));
    assert_eq!((v["spd_rng_min"].as_u64(), v["spd_rng_max"].as_u64()), (Some(1), Some(4)));
    assert_eq!(v["pr_modes"], serde_json::json!(["Auto", "Party", "Quiet"]));
    let number = payload(&DEV, entity("bypass_target"));
    assert_eq!((number["min"].as_f64(), number["max"].as_f64(), number["mode"].as_str()), (Some(10.0), Some(35.0), Some("box")));
}

#[test]
fn enum_options_cover_all_values() {
    let names = |e: &str| -> Vec<String> {
        payload(&DEV, entity(e))["ops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap().into())
            .collect()
    };
    let frost = names("frost_state");
    for state in [
        FrostState::Idle,
        FrostState::Armed,
        FrostState::PreheatWait,
        FrostState::BoostStart,
        FrostState::BoostCheck,
        FrostState::BoostHold,
        FrostState::DefrostCheck,
        FrostState::Defrost,
        FrostState::TestRun,
    ] {
        assert!(frost.contains(&serde_json::to_value(state).unwrap().as_str().unwrap().into()));
    }
    let modes = names("mode");
    for m in [Mode::Manual, Mode::Auto, Mode::Party, Mode::Quiet, Mode::Holiday] {
        // The template capitalises the API value ("party" → "Party").
        let id = std::string::String::from(serde_json::to_value(m).unwrap().as_str().unwrap());
        let shown = std::format!("{}{}", id[..1].to_uppercase(), &id[1..]);
        assert!(modes.contains(&shown), "{shown}");
    }
    assert_eq!((frost.len(), modes.len()), (9, 5));
}

#[test]
fn worst_case_discovery_fits() {
    let long = |c: char, n: usize| -> String { core::iter::repeat_n(c, n).collect() };
    let (node, name, ver, base, prefix, url) =
        (long('n', 23), long('d', 32), long('v', 16), long('b', 48), long('p', 48), long('u', 40));
    // `DeviceIdentity::model` holds 24 bytes; control characters escape to 6 bytes each.
    let model = long('\u{1}', 24);
    let dev = Device {
        node_id: &node,
        name: &name,
        sw_version: &ver,
        model: &model,
        mac: "aa:bb:cc:dd:ee:ff",
        configuration_url: Some(&url),
        base_topic: &base,
        discovery_prefix: &prefix,
    };
    for e in ENTITIES {
        payload(&dev, e);
        dev.discovery_topic(e).expect("topic fits");
    }
}

#[test]
fn too_small_buffer_is_an_error() {
    let mut buf = [0u8; 64];
    assert_eq!(DEV.discovery_payload(entity("fan"), &mut buf), Err(Error::TooLong));
}

#[test]
fn state_json_from_status() {
    let mut buf = [0u8; STATE_JSON_MAX];
    let json = State::new(&status(), &bypass()).to_json(&mut buf).unwrap();
    let v: Value = serde_json::from_slice(json).unwrap();
    assert_eq!(
        (v["fan"].as_str(), v["level"].as_u64(), v["preset"].as_str(), v["mode"].as_str()),
        (Some("ON"), Some(4), Some("Party"), Some("party"))
    );
    assert_eq!(v["t_outdoor"], -8.5);
    assert_eq!(v["auto_return_min"], 2, "61 s rounds up to 2 min");
    assert!(v["t_exhaust"].is_null());
    assert_eq!(
        (v["bypass"].as_str(), v["bypass_enabled"].as_str(), v["bypass_target"].as_f64()),
        (Some("OFF"), Some("ON"), Some(22.0))
    );
    assert_eq!((v["filter_days"].as_u64(), v["filter_due"].as_str()), (Some(50), Some("OFF")));
    assert_eq!(v["problem"], "ON");
    assert_eq!(v["faults"], serde_json::json!(["exhaust_sensor_open"]));

    let mut s = status();
    s.control = Control { mode: Mode::Holiday, level: 0, remaining_s: None, auto_return_s: None, ..s.control };
    s.faults = heapless::Vec::from_slice(&[Fault::FilterChange, Fault::TimeNotSet]).unwrap();
    let v: Value = serde_json::from_slice(State::new(&s, &bypass()).to_json(&mut buf).unwrap()).unwrap();
    assert_eq!((v["fan"].as_str(), v["preset"].as_str(), v["mode"].as_str()), (Some("OFF"), Some(PRESET_NONE), Some("holiday")));
    assert_eq!(v["problem"], "OFF", "notices are not problems");
}

#[test]
fn worst_case_state_fits() {
    let mut s = status();
    let x = -1.234_567_9e-12_f32;
    s.temperatures = Temperatures { outdoor: Some(x), supply: Some(x), extract: Some(x), exhaust: Some(x) };
    s.efficiency_pct = Some(x);
    s.heat_recovery = HeatRecovery { power_w: Some(x), heating_kwh: x, cooling_kwh: x };
    s.supply_fan = Fan { volts: x, duty_pct: 100, rpm: u32::MAX, stalled: true, airflow_m3h: Some(-1.234_567_9e-12) };
    s.exhaust_fan = s.supply_fan;
    s.filter.remaining_days = u32::MAX;
    s.operating_hours = u32::MAX;
    s.control.mode = Mode::Holiday;
    s.control.auto_return_s = Some(u32::MAX);
    s.frost.state = FrostState::DefrostCheck;
    s.faults.clear();
    while s.faults.push(Fault::ExtractSensorShort).is_ok() {}
    let b = BypassConfig { target_c: x, ..bypass() };
    let mut buf = [0u8; STATE_JSON_MAX];
    State::new(&s, &b).to_json(&mut buf).expect("fits");
}

#[test]
fn command_parsing() {
    let p = |topic: &str, payload: &str| Command::parse("openkwl", topic, payload.as_bytes());
    assert_eq!(p("openkwl/fan/set", "ON"), Ok(Command::On));
    assert_eq!(p("openkwl/fan/set", "OFF\n"), Ok(Command::Off));
    assert_eq!(p("openkwl/fan/level/set", "0"), Ok(Command::Level(0)));
    assert_eq!(p("openkwl/fan/level/set", "4"), Ok(Command::Level(4)));
    assert_eq!(p("openkwl/fan/preset/set", "auto"), Ok(Command::Preset(Mode::Auto)));
    assert_eq!(p("openkwl/fan/preset/set", "party"), Ok(Command::Preset(Mode::Party)));
    assert_eq!(p("openkwl/fan/preset/set", "Quiet"), Ok(Command::Preset(Mode::Quiet)));
    assert_eq!(p("openkwl/fan/preset/set", "None"), Ok(Command::Preset(Mode::Manual)));
    assert_eq!(p("openkwl/bypass/enabled/set", "OFF"), Ok(Command::BypassEnabled(false)));
    assert_eq!(p("openkwl/bypass/target/set", "22.5"), Ok(Command::BypassTarget(22.5)));
    assert_eq!(p("openkwl/filter/reset", "PRESS"), Ok(Command::FilterReset));
    assert_eq!(p("openkwl/system/reboot", "PRESS"), Ok(Command::Reboot));

    assert_eq!(p("openkwl/fan/set", "on"), Err(Error::BadPayload));
    assert_eq!(p("openkwl/fan/level/set", "5"), Err(Error::BadPayload));
    assert_eq!(p("openkwl/fan/level/set", "-1"), Err(Error::BadPayload));
    assert_eq!(p("openkwl/fan/preset/set", "holiday"), Err(Error::BadPayload), "holiday needs dates");
    assert_eq!(p("openkwl/bypass/target/set", "NaN"), Err(Error::BadPayload));
    assert_eq!(p("openkwl/bypass/target/set", "inf"), Err(Error::BadPayload));
    assert_eq!(p("openkwl/system/reboot", "ON"), Err(Error::BadPayload));
    assert_eq!(Command::parse("openkwl", "openkwl/fan/set", &[0xff]), Err(Error::BadPayload));
    assert_eq!(p("openkwlx/fan/set", "ON"), Err(Error::UnknownTopic), "prefix must end at a level");
    assert_eq!(p("openkwl/state", "{}"), Err(Error::UnknownTopic));
}

#[test]
fn fan_commands_become_valid_requests() {
    for cmd in [
        Command::On,
        Command::Off,
        Command::Level(2),
        Command::Preset(Mode::Auto),
        Command::Preset(Mode::Party),
        Command::Preset(Mode::Quiet),
        Command::Preset(Mode::Manual),
    ] {
        assert!(cmd.to_request(3).expect("control command").validate().is_ok(), "{cmd:?}");
    }
    assert_eq!(Command::On.to_request(0).unwrap().level, Some(1), "ON never means level 0");
    assert_eq!(Command::Off.to_request(3).unwrap().level, Some(0));
    for settings in [Command::BypassEnabled(true), Command::BypassTarget(20.0), Command::FilterReset, Command::Reboot] {
        assert!(settings.to_request(3).is_none());
    }
}

#[test]
fn change_detection() {
    let base = State::new(&status(), &bypass());
    assert_eq!(base.change_from(&base), Change::None);

    let mut s = base.clone();
    s.t_outdoor = s.t_outdoor.map(|t| t + 0.1);
    assert_eq!(s.change_from(&base), Change::None, "inside the dead band");
    s.t_outdoor = s.t_outdoor.map(|t| t + 0.2);
    assert_eq!(s.change_from(&base), Change::Measurement);

    let mut s = base.clone();
    s.supply_rpm += 49;
    assert_eq!(s.change_from(&base), Change::None);
    s.supply_rpm += 1;
    assert_eq!(s.change_from(&base), Change::Diagnostic);
    s.t_outdoor = s.t_outdoor.map(|t| t + 0.3);
    assert_eq!(s.change_from(&base), Change::Measurement, "the larger change counts");

    let mut s = base.clone();
    s.bypass = OnOff::On;
    assert_eq!(s.change_from(&base), Change::State);
    let mut s = base.clone();
    s.level = 1;
    assert_eq!(s.change_from(&base), Change::State);
    let mut s = base.clone();
    s.t_supply = None;
    assert_eq!(s.change_from(&base), Change::State, "sensor lost");
}

#[test]
fn hand_written_json_matches_serde() {
    let mut a = [0u8; DISCOVERY_JSON_MAX];
    let mut b = [0u8; DISCOVERY_JSON_MAX];
    for dev in [DEV, Device { mac: "", configuration_url: None, name: "K\"W\\L", model: "", ..DEV }] {
        for e in ENTITIES {
            let new = dev.discovery_payload(e, &mut a).unwrap();
            let old = dev.discovery_payload_serde(e, &mut b).unwrap();
            assert_eq!(core::str::from_utf8(new).unwrap(), core::str::from_utf8(old).unwrap(), "{}", e.object_id);
        }
    }
    let mut a = [0u8; STATE_JSON_MAX];
    let mut b = [0u8; STATE_JSON_MAX];
    let mut s = status();
    let full = State::new(&s, &bypass());
    s.temperatures.outdoor = None;
    let mut sparse = State::new(&s, &bypass());
    sparse.auto_return_min = Some(17);
    sparse.heat_w = Some(-12.5);
    for st in [full, sparse] {
        let new = st.to_json(&mut a).unwrap();
        let old = st.to_json_serde(&mut b).unwrap();
        assert_eq!(core::str::from_utf8(new).unwrap(), core::str::from_utf8(old).unwrap());
    }
}

#[test]
fn model_comes_from_the_device_identity() {
    let mdl = |model: &str| String::from(payload(&Device { model, ..DEV }, entity("fan"))["dev"]["mdl"].as_str().unwrap());
    assert_eq!(mdl("KWL EC 300W R"), "KWL EC 300W R");
    assert_eq!(mdl("KWL EC 500W L"), "KWL EC 500W L");
    assert_eq!(mdl(""), UNKNOWN_MODEL, "not imported");
    assert_eq!(mdl("  "), UNKNOWN_MODEL);
    assert_eq!(payload(&DEV, entity("fan"))["dev"]["mf"], "Helios");
}
