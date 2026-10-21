extern crate std;

use std::string::String;

use openkwl_api::time::{Date, TimeOfDay};
use openkwl_core::clock;

use super::*;
use crate::{CONFIG_MAX_LEN, Config, Secrets};

fn render(c: &Config) -> String {
    let mut out = String::new();
    write_config(c, &mut out).unwrap();
    out
}

fn md(m: u8, d: u8) -> MonthDay {
    MonthDay(clock::MonthDay::new(m, d).unwrap())
}

/// A config with every optional field set and values different from the defaults.
fn full() -> Config {
    let mut c = Config::default();
    c.network.dhcp = false;
    c.network.hostname = "Helios".try_into().unwrap();
    c.network.address = Some("192.168.1.50".try_into().unwrap());
    c.network.netmask = Some("255.255.255.0".try_into().unwrap());
    c.network.gateway = Some("192.168.1.1".try_into().unwrap());
    c.network.dns = Some("192.168.1.2".try_into().unwrap());
    c.time.ntp_server = "time.example.net".try_into().unwrap();
    c.mqtt.enabled = true;
    c.mqtt.host = "broker.lan".try_into().unwrap();
    c.mqtt.username = "kwl".try_into().unwrap();
    c.fans.supply_volts = [5.0, 6.5, 7.25, 9.0];
    c.fans.airflow_m3h = [90, 140, 190, 260];
    c.bypass.season = Some(Season { start: md(3, 1), end: md(10, 2) });
    c.bypass.actuator = ActuatorType::MotorReversed;
    c.frost.arm_below_c = -3.1;
    c.holiday = openkwl_api::HolidayConfig {
        enabled: true,
        start: Date(clock::Date::new(2024, 7, 13).unwrap()),
        end: Date(clock::Date::new(2024, 7, 19).unwrap()),
        level: 1,
        interval_h: 2,
        active_min: 60,
    };
    c.schedule.auto_return_min = Some(120);
    let slot = |h, m, level| Slot { from: TimeOfDay::new(h, m).unwrap(), level };
    c.profiles[0].days[0] =
        heapless::Vec::from_slice(&[slot(0, 0, 1), slot(6, 30, 3), slot(8, 0, 1), slot(16, 0, 2), slot(22, 30, 1)]).unwrap();
    c.profiles[0].days[6] = heapless::Vec::new();
    c.device.mac = "80:4B:20:12:34:56".try_into().unwrap();
    c.device.identity.serial = "1000".try_into().unwrap();
    c
}

#[test]
fn defaults_round_trip() {
    let c = Config::default();
    assert!(validate(&c).is_ok());
    let (back, report) = read_config(&render(&c), Config::default()).unwrap();
    assert_eq!(back, c);
    assert!(report.warnings.is_empty());
}

#[test]
fn full_config_round_trips() {
    let c = full();
    let text = render(&c);
    let (back, report) = read_config(&text, Config::default()).unwrap();
    assert_eq!(back, c, "{text}");
    assert!(report.warnings.is_empty());
    assert!(text.contains("mon = 00:00=1, 06:30=3, 08:00=1, 16:00=2, 22:30=1\n"), "{text}");
    assert!(text.contains("sun =\n"));
    assert!(text.contains("season = 03-01..10-02\n"));
    assert!(text.contains("supply_volts = 5.0, 6.5, 7.25, 9.0\n"), "{text}");
    assert!(text.contains("arm_below_c = -3.1\n"));
}

#[test]
fn worst_case_fits_the_read_buffer() {
    let mut c = full();
    let slots: heapless::Vec<Slot, MAX_SLOTS> = (0..MAX_SLOTS as u16)
        .map(|i| Slot { from: TimeOfDay::new(10 + i, 59).unwrap(), level: 4 })
        .collect();
    for p in c.profiles.iter_mut() {
        p.days = core::array::from_fn(|_| slots.clone());
    }
    c.mqtt.host = "h".repeat(64).as_str().try_into().unwrap();
    c.time.ntp_server = "n".repeat(64).as_str().try_into().unwrap();
    let text = render(&c);
    assert!(text.len() < CONFIG_MAX_LEN, "{} bytes", text.len());
    assert_eq!(read_config(&text, Config::default()).unwrap().0, c);
}

#[test]
fn missing_keys_keep_defaults() {
    let (c, _) = read_config("[bypass]\ntarget_c = 24.5\n", Config::default()).unwrap();
    assert_eq!(c.bypass.target_c, 24.5);
    assert_eq!(c.bypass.min_outdoor_c, Config::default().bypass.min_outdoor_c);
}

#[test]
fn comments_crlf_and_bom() {
    let text = "\u{feff}# head\r\n; also a comment\r\n[mqtt]\r\nbase_topic = helios/kwl   # topic\r\n\r\n[fans]\r\nairflow_m3h = 1,2, 3 ,4\r\n";
    let (c, r) = read_config(text, Config::default()).unwrap();
    assert_eq!(c.mqtt.base_topic, "helios/kwl");
    assert_eq!(c.fans.airflow_m3h, [1, 2, 3, 4]);
    assert!(r.warnings.is_empty());
}

#[test]
fn unknown_keys_and_sections_warn() {
    let text = "orphan = 1\n[future]\nx = 1\n[bypass]\nnew_option = 3\ntarget_c = 21\n";
    let (c, r) = read_config(text, Config::default()).unwrap();
    assert_eq!(c.bypass.target_c, 21.0);
    let lines: std::vec::Vec<_> = r.warnings.iter().map(|w| (w.line, w.kind)).collect();
    assert_eq!(
        lines,
        [(1, WarningKind::UnknownKey), (2, WarningKind::UnknownSection), (3, WarningKind::UnknownKey), (5, WarningKind::UnknownKey)]
    );
}

#[test]
fn errors_carry_line_numbers() {
    let err = |t: &str| read_config(t, Config::default()).unwrap_err();
    assert_eq!(err("[bypass]\n\ntarget_c = warm\n"), Error { line: 3, kind: ErrorKind::BadValue });
    assert_eq!(err("[fans]\nsupply_volts = 1, 2, 3\n").kind, ErrorKind::BadValue, "too few");
    assert_eq!(err("[fans]\nsupply_volts = 1, 2, 3, 4, 5\n").kind, ErrorKind::BadValue, "too many");
    assert_eq!(err("[profile1]\nmon = 06:30 3\n").kind, ErrorKind::BadValue);
    assert_eq!(err("[profile1]\nmon = 24:00=3\n").kind, ErrorKind::BadValue);
    assert_eq!(err("[bypass]\nseason = 03-01\n").kind, ErrorKind::BadValue);
    assert_eq!(err("[bypass]\nactuator = magic\n").kind, ErrorKind::BadValue);
    assert_eq!(err(&(String::from("[network]\nhostname = ") + &"x".repeat(33))).kind, ErrorKind::BadValue);
    assert_eq!(err("[network\n"), Error { line: 1, kind: ErrorKind::Syntax });
    assert_eq!(err("just text\n").kind, ErrorKind::Syntax);
}

#[test]
fn validation_runs_after_parsing() {
    let e = read_config("[profile2]\ntue = 08:00=1, 07:00=2\n", Config::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Invalid("profile2", FieldError { field: "days", .. })), "{e:?}");
    let e = read_config("[network]\ndhcp = false\n", Config::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Invalid("network", FieldError { field: "address", .. })));
    let e = read_config("[fans]\nsupply_volts = 5, 6, 7, 12\n", Config::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Invalid("fans", _)));
    // A hand-edited frost value outside the API limits is rejected, not taken silently.
    let e = read_config("[frost]\ndefrost_s = 86400\n", Config::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Invalid("frost", FieldError { field: "defrost_s", .. })), "{e:?}");
    let e = read_config("[frost]\nexhaust_boost_v = 5\n", Config::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Invalid("frost", FieldError { field: "exhaust_boost_v", .. })), "{e:?}");
    assert!(read_config("[frost]\ndefrost_s = 14400\n", Config::default()).is_ok());
}

#[test]
fn secrets_allow_hash_and_round_trip() {
    let s = Secrets { mqtt_password: "p#ss word # x".try_into().unwrap() };
    let mut text = String::new();
    write_secrets(&s, &mut text).unwrap();
    assert_eq!(read_secrets(&text).unwrap(), s);
    assert_eq!(read_secrets("").unwrap(), Secrets::default());
    assert_eq!(read_secrets("[other]\npassword = x\n").unwrap(), Secrets::default(), "only [mqtt]");
}

#[test]
fn floats_are_written_like_ryu() {
    let check = |v: f32| {
        let s = std::format!("{}", F(v));
        assert_eq!(s, ryu::Buffer::new().format(v), "bits {:#010x}", v.to_bits());
        if v.is_finite() {
            assert_eq!(s.parse::<f32>().map(f32::to_bits), Ok(v.to_bits()), "{s}");
        }
        // The reader's parser is exact in the range settings use.
        if (1e-3..1e7).contains(&v.abs()) {
            assert_eq!(openkwl_core::num::parse_f32(&s).map(f32::to_bits), Some(v.to_bits()), "{s}");
        }
    };
    for (v, s) in [(21.0, "21.0"), (-8.25, "-8.25"), (0.0, "0.0"), (-0.0, "-0.0"), (75.26881, "75.26881"), (1e-7, "1e-7")] {
        assert_eq!(std::format!("{}", F(v)), s);
    }
    for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN_POSITIVE, f32::from_bits(1)] {
        check(v);
    }
    for i in -1000..=1000 {
        check(i as f32 / 10.0);
        check(i as f32 / 4.0);
    }
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        check(f32::from_bits(x as u32));
    }
}
