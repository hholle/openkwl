//! The reader against `serde-json-core` (what the firmware used before): for every request type
//! and many bodies, both must accept with equal values, or both refuse.

extern crate std;

use core::fmt::Debug;

use serde::de::DeserializeOwned;
use std::{format, string::String, vec, vec::Vec};

use super::{FromJson, from_slice};
use crate::Validate;
use crate::{
    BypassConfig, ControlRequest, FanConfig, FilterConfig, FrostConfig, HolidayConfig, MqttConfigUpdate, NetworkConfig,
    ProgramConfig, ScheduleConfig, ScheduleProfile, SetTimeRequest, TimeConfig,
};

fn serde<T: DeserializeOwned>(body: &str) -> Option<T> {
    let mut scratch = [0u8; 4096];
    serde_json_core::from_slice_escaped::<T>(body.as_bytes(), &mut scratch).ok().map(|(v, _)| v)
}

/// Same verdict after validation (what the API does with a body) and, when accepted, the same
/// value. (serde-json-core reads `null` as NaN for floats, which validation then refuses; this
/// reader refuses it at once.)
fn agree<T: FromJson + DeserializeOwned + Validate + PartialEq + Debug>(bodies: &[String]) {
    for body in bodies {
        let ours = from_slice::<T>(body.as_bytes()).ok().filter(|v| v.validate().is_ok());
        let theirs = serde::<T>(body).filter(|v| v.validate().is_ok());
        assert_eq!(ours, theirs, "{} {body}", core::any::type_name::<T>());
    }
}

/// A valid body, then variants: each key missing, each value replaced by odd ones, extra keys,
/// whitespace, trailing data, a duplicate key.
fn variants(valid: &str, odd: &[&str]) -> Vec<String> {
    let mut out = vec![
        valid.into(),
        format!(" \n{valid}\t "),
        format!("{valid}x"),
        format!(r#"{{"unknown":{{"a":[1,"b",null]}},{}"#, &valid[1..]),
    ];
    // Each top-level "key":value pair removed, duplicated, and its value replaced.
    let inner = &valid[1..valid.len() - 1];
    let pairs = split_top(inner);
    for (i, p) in pairs.iter().enumerate() {
        let without: Vec<&str> = pairs.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect();
        out.push(format!("{{{}}}", without.join(",")));
        out.push(format!("{{{inner},{p}}}"));
        let key = &p[..p.find(':').unwrap()];
        for o in odd {
            let replaced: Vec<String> = pairs
                .iter()
                .enumerate()
                .map(|(j, q)| if j == i { format!("{key}:{o}") } else { (*q).into() })
                .collect();
            out.push(format!("{{{}}}", replaced.join(",")));
        }
    }
    out
}

/// Splits `"a":1,"b":[1,2],"c":{"d":3}` at its top-level commas.
fn split_top(s: &str) -> Vec<&str> {
    let (mut depth, mut in_str, mut start, mut out) = (0i32, false, 0, Vec::new());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'{' | b'[' if !in_str => depth += 1,
            b'}' | b']' if !in_str => depth -= 1,
            b',' if !in_str && depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

const ODD: &[&str] = &[
    "null",
    "true",
    "0",
    "-1",
    "1.5",
    "300",
    "70000",
    "-40000",
    "4294967296",
    "1e3",
    "\"x\"",
    "\"\"",
    "[]",
    "{}",
    "\"motor\"",
    "\"03-01\"",
    "\"2026-07-13\"",
    "\"06:30\"",
    "\"a\\\"b\\\\c\\u00e4\\t\"",
    "\"\\u0000\"",
    "\"bad\\q\"",
    "[1,2,3,4]",
    "[1.5,2,3,4]",
    "[1,2,3]",
];

#[test]
fn bypass() {
    agree::<BypassConfig>(&variants(
        r#"{"enabled":true,"target_c":22.5,"min_outdoor_c":7,"close_if_outdoor_warmer":false,"actuator":"motor_reversed","run_time_s":120,"season":{"start":"03-01","end":"10-31"},"supply_offset_k":4}"#,
        ODD,
    ));
    agree::<BypassConfig>(&variants(
        r#"{"enabled":false,"target_c":-1e1,"min_outdoor_c":0.25,"close_if_outdoor_warmer":true,"actuator":"analog","run_time_s":5,"season":null,"supply_offset_k":-3.5}"#,
        ODD,
    ));
}

#[test]
fn frost_fans_programs_filter() {
    agree::<FrostConfig>(&variants(
        r#"{"arm_below_c":-3.1,"entry_pct":58,"hold_pct":60,"exit_pct":60,"boost_delay_s":900,"defrost_s":10800,"test_run_s":900,"exhaust_boost_v":1.2}"#,
        ODD,
    ));
    agree::<FanConfig>(&variants(
        r#"{"supply_volts":[5,6,7.5,9],"extract_volts":[5.0,6.0,7.0,9.0],"airflow_m3h":[100,150,200,250],"m3h_per_krpm":118}"#,
        ODD,
    ));
    agree::<ProgramConfig>(&variants(r#"{"party_level":4,"party_min":150,"quiet_level":0,"quiet_min":180}"#, ODD));
    agree::<FilterConfig>(&variants(r#"{"interval_months":3}"#, ODD));
}

#[test]
fn holiday_time_network_schedule() {
    agree::<HolidayConfig>(&variants(
        r#"{"enabled":true,"start":"2026-07-13","end":"2026-07-19","level":1,"interval_h":2,"active_min":60}"#,
        ODD,
    ));
    agree::<TimeConfig>(&variants(r#"{"ntp_enabled":true,"ntp_server":"pool.ntp.org","utc_offset_min":-60,"eu_dst":true}"#, ODD));
    agree::<NetworkConfig>(&variants(
        r#"{"dhcp":false,"hostname":"openkwl","address":"192.168.1.50","netmask":"255.255.255.0","gateway":null,"dns":"192.168.1.1"}"#,
        ODD,
    ));
    agree::<ScheduleConfig>(&variants(
        r#"{"active_profile":2,"auto_at_midnight":true,"auto_return_min":null,"fallback_level":2}"#,
        ODD,
    ));
    agree::<ScheduleConfig>(&variants(
        r#"{"active_profile":5,"auto_at_midnight":false,"auto_return_min":90,"fallback_level":0}"#,
        ODD,
    ));
}

#[test]
fn mqtt_with_escapes() {
    agree::<MqttConfigUpdate>(&variants(
        r#"{"enabled":true,"host":"broker.lan","port":1883,"username":"k\u00e4l \"x\"","password":"p\\w\/d","client_id":"openkwl","base_topic":"openkwl","discovery":true,"discovery_prefix":"homeassistant"}"#,
        ODD,
    ));
    agree::<MqttConfigUpdate>(&variants(
        r#"{"enabled":false,"host":"","port":1,"username":"","client_id":"x","base_topic":"a","discovery":false,"discovery_prefix":"b"}"#,
        ODD,
    ));
}

#[test]
fn profiles_control_time() {
    let day = r#"[{"from":"00:00","level":1},{"from":"06:30","level":3},{"from":"22:30","level":1}]"#;
    let days = [day; 7].join(",");
    agree::<ScheduleProfile>(&variants(&format!(r#"{{"days":[{days}]}}"#), ODD));
    agree::<ScheduleProfile>(&[
        format!(r#"{{"days":[{}]}}"#, [day; 6].join(",")),
        format!(r#"{{"days":[{days},[]]}}"#),
        r#"{"days":[[],[],[],[],[],[],[]]}"#.into(),
    ]);
    agree::<ControlRequest>(&variants(r#"{"mode":"party","level":4,"duration_min":30,"end_program":false}"#, ODD));
    agree::<ControlRequest>(&[
        "{}".into(),
        r#"{"level":3}"#.into(),
        r#"{"end_program":true}"#.into(),
        r#"{"mode":"holidays"}"#.into(),
    ]);
    agree::<SetTimeRequest>(&variants(r#"{"unix_s":1792186862}"#, ODD));
}

#[test]
fn malformed_bodies() {
    for body in [
        "",
        "{",
        "{\"interval_months\":3",
        "{\"interval_months\" 3}",
        "{\"interval_months\":3,}",
        "[3]",
        "null",
        "{\"interval_months\":03}",
        "{interval_months:3}",
    ] {
        agree::<FilterConfig>(&[body.into()]);
    }
}

#[test]
fn decodes_what_serde_json_core_cannot() {
    // A surrogate pair (an emoji written as \uD83D\uDE00): serde-json-core refuses it.
    let t: crate::TimeConfig =
        from_slice(br#"{"ntp_enabled":true,"ntp_server":"x\ud83d\ude00","utc_offset_min":60,"eu_dst":true}"#).unwrap();
    assert_eq!(t.ntp_server.as_str(), "x\u{1F600}");
    assert!(
        from_slice::<crate::TimeConfig>(br#"{"ntp_enabled":true,"ntp_server":"x\ud83d","utc_offset_min":60,"eu_dst":true}"#)
            .is_err(),
        "lone surrogate"
    );
}
