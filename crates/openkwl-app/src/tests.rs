extern crate std;

use std::{
    format,
    string::{String, ToString},
};

use openkwl_config::{Config, Secrets, state::RuntimeState};
use openkwl_core::{clock, ntc::Reading};
use rand_core::{CryptoRng, RngCore};
use serde_json::{Value, json};

use super::*;

struct TestRng(u64);
impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for b in dest {
            *b = self.next_u64() as u8;
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl CryptoRng for TestRng {}

const PASSWORD: &str = "correct horse battery";
/// 2026-10-19 (Monday) 10:00 UTC = 12:00 CEST.
const MONDAY_NOON: u64 = 1_792_404_000;

fn boot() -> Boot {
    Boot {
        config: Config::default(),
        secrets: Secrets::default(),
        auth: None,
        state: RuntimeState { auto_mode: true, manual_level: 2, ..Default::default() },
        config_invalid: false,
        platform: PlatformInfo::default(),
    }
}

/// Mild autumn day, fans running.
fn inputs() -> Inputs {
    Inputs {
        outdoor: Reading::Celsius(12.8),
        supply: Reading::Celsius(19.8),
        extract: Reading::Celsius(22.1),
        exhaust: Reading::Celsius(13.7),
        supply_rpm: 1350,
        exhaust_rpm: 1310,
    }
}

struct Harness {
    app: App,
    rng: TestRng,
    token: Option<String>,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new(boot());
        app.set_connectivity(Connectivity { sd_card: true, ..Default::default() });
        Self { app, rng: TestRng(42), token: None }
    }

    fn logged_in() -> Self {
        let mut h = Self::new();
        let (s, v) = h.call(Method::Post, "/setup", Some(json!({ "password": PASSWORD })));
        assert_eq!(s, 200, "{v}");
        h.token = Some(v["token"].as_str().unwrap().into());
        h
    }

    fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let (status, text) = self.raw(method, path, body.map(|b| b.to_string()));
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    fn raw(&mut self, method: Method, path: &str, body: Option<String>) -> (u16, String) {
        let path = format!("/api/v1{path}");
        let auth = self.token.as_ref().map(|t| format!("Bearer {t}"));
        let body = body.unwrap_or_default();
        let req = Request {
            method,
            path: &path,
            authorization: auth.as_deref(),
            client: 0,
            host: None,
            json: true,
            body: body.as_bytes(),
        };
        let mut out = [0u8; 16 * 1024];
        let r = self.app.handle(&req, &mut self.rng, &mut out);
        (r.status, String::from_utf8(out[..r.len].to_vec()).unwrap())
    }

    fn tick(&mut self, seconds: u64, unix: Option<u64>, i: &Inputs) -> Outputs {
        let mut out = Outputs::default();
        for s in 0..seconds {
            out = self.app.tick_second(i, unix.map(|t| t + s));
        }
        out
    }
}

#[test]
fn setup_flow_and_auth() {
    let mut h = Harness::new();
    let (s, v) = h.call(Method::Get, "/setup", None);
    assert_eq!((s, v["setup_required"].as_bool()), (200, Some(true)));
    assert_eq!(h.call(Method::Get, "/status", None).0, 403, "setup first");
    let (s, v) = h.call(Method::Post, "/setup", Some(json!({ "password": "short" })));
    assert_eq!((s, v["field"].as_str()), (422, Some("password")));
    let (s, v) = h.call(Method::Post, "/setup", Some(json!({ "password": PASSWORD })));
    assert_eq!(s, 200);
    assert_eq!(v["token"].as_str().unwrap().len(), 64);
    assert!(h.app.take_writes().auth, "AUTH.BIN must be written");
    assert_eq!(h.call(Method::Post, "/setup", Some(json!({ "password": PASSWORD }))).0, 403, "only once");
    assert_eq!(h.call(Method::Get, "/status", None).0, 401, "no token");
    h.token = Some(v["token"].as_str().unwrap().into());
    assert_eq!(h.call(Method::Get, "/status", None).0, 200);
    assert_eq!(h.call(Method::Get, "/nope", None).0, 404);
}

#[test]
fn setup_window_closes() {
    let mut h = Harness::new();
    h.tick(SETUP_WINDOW_S, None, &inputs());
    let (s, v) = h.call(Method::Post, "/setup", Some(json!({ "password": PASSWORD })));
    assert_eq!((s, v["error"].as_str()), (403, Some("setup_required")));
    assert_eq!(h.call(Method::Get, "/setup", None).1["window_remaining_s"], 0);
}

#[test]
fn login_logout_and_throttle() {
    let mut h = Harness::logged_in();
    h.token = None;
    for _ in 0..3 {
        assert_eq!(h.call(Method::Post, "/auth/login", Some(json!({ "password": "wrong one!" }))).0, 401);
    }
    assert_eq!(h.call(Method::Post, "/auth/login", Some(json!({ "password": "wrong one!" }))).0, 401);
    let (s, v) = h.call(Method::Post, "/auth/login", Some(json!({ "password": PASSWORD })));
    assert_eq!((s, v["retry_after_s"].as_u64()), (429, Some(5)), "locked even for the right password");
    h.tick(5, None, &inputs());
    let (s, v) = h.call(Method::Post, "/auth/login", Some(json!({ "password": PASSWORD })));
    assert_eq!(s, 200);
    h.token = Some(v["token"].as_str().unwrap().into());
    assert_eq!(h.call(Method::Post, "/auth/logout", None).0, 204);
    assert_eq!(h.call(Method::Get, "/status", None).0, 401, "token revoked");
}

#[test]
fn password_change_revokes_sessions() {
    let mut h = Harness::logged_in();
    let (s, _) = h.call(
        Method::Post,
        "/auth/password",
        Some(json!({ "current_password": "bad bad bad", "new_password": "another long one" })),
    );
    assert_eq!(s, 401);
    let (s, _) =
        h.call(Method::Post, "/auth/password", Some(json!({ "current_password": PASSWORD, "new_password": "another long one" })));
    assert_eq!(s, 204);
    assert_eq!(h.call(Method::Get, "/status", None).0, 401);
    assert!(h.app.auth_record().unwrap().verify(b"another long one"));
}

#[test]
fn weekly_program_drives_the_fans() {
    let mut h = Harness::logged_in();
    // Default profile 1: level 2 all day = 6 V.
    let out = h.tick(1, Some(MONDAY_NOON), &inputs());
    assert_eq!((out.supply_volts, out.exhaust_volts, out.supply_duty), (6.0, 6.0, 60));
    let st = h.app.status();
    assert_eq!((st.control.mode, st.control.level), (openkwl_api::Mode::Auto, 2));
    assert_eq!(st.time.map(|t| t.second_of_day), Some(12 * 3600));
    assert!((st.efficiency_pct.unwrap() - 75.27).abs() < 0.1);

    // Monday 12:00 in a custom profile: 11:30 slot at level 4.
    let (s, _) = h.call(
        Method::Put,
        "/config/schedule/1",
        Some(json!({ "days": [[{"from":"00:00","level":1},{"from":"11:30","level":4}],[],[],[],[],[],[]] })),
    );
    assert_eq!(s, 200);
    let out = h.tick(1, Some(MONDAY_NOON + 1), &inputs());
    assert_eq!(out.supply_volts, 9.0);

    // Days without slots continue Monday's last level, not the "clock not set" fallback.
    let out = h.tick(1, Some(MONDAY_NOON + 2 * 86_400), &inputs());
    assert_eq!((out.supply_volts, h.app.control().level), (9.0, 4), "Wednesday");
    let today = h.app.status().schedule.unwrap();
    assert_eq!((today.today.len(), today.today[0].level, today.next), (1, 4, None), "carried in; next change only next week");
    let today = h.tick(1, Some(MONDAY_NOON + 6 * 86_400), &inputs()).supply_volts;
    assert_eq!(today, 9.0, "Sunday");
    let next = h.app.status().schedule.unwrap().next.unwrap();
    assert_eq!((next.at.0, next.level, next.tomorrow), (0, 1, true));
}

#[test]
fn without_clock_auto_uses_the_fallback_level() {
    let mut h = Harness::logged_in();
    h.app.config.schedule.fallback_level = 1;
    let out = h.tick(1, None, &inputs());
    assert_eq!(out.supply_volts, 5.0);
    assert!(h.app.status().faults.contains(&openkwl_api::Fault::TimeNotSet));
}

#[test]
fn manual_party_and_auto_return() {
    let mut h = Harness::logged_in();
    let (s, _) = h.call(
        Method::Put,
        "/config/schedule",
        Some(json!({ "active_profile": 1, "auto_at_midnight": true, "auto_return_min": 10, "fallback_level": 2 })),
    );
    assert_eq!(s, 200);
    h.tick(1, Some(MONDAY_NOON), &inputs());

    let (s, v) = h.call(Method::Patch, "/control", Some(json!({ "level": 4 })));
    assert_eq!((s, v["mode"].as_str(), v["level"].as_u64(), v["auto_return_s"].as_u64()), (200, Some("manual"), Some(4), Some(600)));
    assert_eq!(h.tick(1, Some(MONDAY_NOON + 1), &inputs()).supply_volts, 9.0);

    let (_, v) = h.call(Method::Patch, "/control", Some(json!({ "mode": "quiet" })));
    assert_eq!((v["mode"].as_str(), v["level"].as_u64(), v["remaining_s"].as_u64()), (Some("quiet"), Some(0), Some(180 * 60)));
    assert_eq!(h.tick(1, Some(MONDAY_NOON + 2), &inputs()).supply_duty, 0, "level 0 = fans off");

    let (_, v) = h.call(Method::Patch, "/control", Some(json!({ "mode": "manual", "level": 3 })));
    assert_eq!(v["mode"], "manual");
    h.tick(600, Some(MONDAY_NOON + 3), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Auto, "returned after 10 min");

    assert_eq!(h.call(Method::Patch, "/control", Some(json!({ "mode": "holiday" }))).0, 422);
    assert_eq!(h.call(Method::Patch, "/control", Some(json!({ "level": 9 }))).0, 422);
    assert_eq!(h.call(Method::Patch, "/control", Some(json!("x"))).0, 400);
}

#[test]
fn midnight_returns_to_auto() {
    let mut h = Harness::logged_in();
    // 23:59:50 CEST on Monday.
    let late = MONDAY_NOON + 12 * 3600 - 10;
    h.tick(1, Some(late), &inputs());
    h.call(Method::Patch, "/control", Some(json!({ "level": 3 })));
    h.tick(5, Some(late + 1), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Manual);
    h.tick(10, Some(late + 6), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Auto);
}

#[test]
fn passwords_can_be_hashed_outside_the_app() {
    let mut h = Harness::logged_in();
    let login = |pw: &str| format!(r#"{{"password":"{pw}"}}"#);
    let path = "/api/v1/auth/login";
    let body = login(PASSWORD);
    let req = Request { method: Method::Post, path, authorization: None, client: 0, host: None, json: true, body: body.as_bytes() };
    let work = h.app.password_work(&req);
    assert!(!work.is_empty());
    let mut out = [0u8; 2048];
    let r = h.app.handle_hashed(&req, &mut h.rng, &mut out, work.run([7; openkwl_auth::SALT_LEN]));
    assert_eq!(r.status, 200);

    let wrong = login("not the password");
    let req = Request { body: wrong.as_bytes(), ..req };
    let hashed = h.app.password_work(&req).run([7; openkwl_auth::SALT_LEN]);
    assert_eq!(h.app.handle_hashed(&req, &mut h.rng, &mut out, hashed).status, 401);

    // No session, no work: a password change from outside never gets hashed.
    let change = r#"{"current_password":"x","new_password":"another long one"}"#;
    let req = Request {
        method: Method::Post,
        path: "/api/v1/auth/password",
        authorization: None,
        client: 0,
        host: None,
        json: true,
        body: change.as_bytes(),
    };
    assert!(h.app.password_work(&req).is_empty());
    let get =
        Request { method: Method::Get, path: "/api/v1/status", authorization: None, client: 0, host: None, json: false, body: b"" };
    assert!(h.app.password_work(&get).is_empty());
}

#[test]
fn setup_and_login_need_json_against_cross_site_requests() {
    // A foreign web page can POST text/plain or a form to the unit without a CORS preflight.
    let mut h = Harness::logged_in();
    let body = format!(r#"{{"password":"{PASSWORD}"}}"#);
    let req = Request {
        method: Method::Post,
        path: "/api/v1/auth/login",
        authorization: None,
        client: 0,
        host: None,
        json: false,
        body: body.as_bytes(),
    };
    assert!(h.app.password_work(&req).is_empty(), "no hashing for it either");
    let mut out = [0u8; 2048];
    let r = h.app.handle_hashed(&req, &mut h.rng, &mut out, Hashed::default());
    assert_eq!(r.status, 400);
}

#[test]
fn password_change_is_throttled_like_login() {
    let mut h = Harness::logged_in();
    let wrong = json!({ "current_password": "not the password", "new_password": "another long one" });
    // The login throttle's free attempts, then a lockout.
    let statuses: std::vec::Vec<u16> = (0..=openkwl_auth::LoginThrottle::FREE_ATTEMPTS + 1)
        .map(|_| h.call(Method::Post, "/auth/password", Some(wrong.clone())).0)
        .collect();
    assert_eq!(statuses.first(), Some(&401));
    assert_eq!(statuses.last(), Some(&429), "{statuses:?}");
}

#[test]
fn first_setup_only_by_the_units_own_name_or_address() {
    use crate::http::local_host;
    for ok in [
        None,
        Some("192.168.1.50"),
        Some("192.168.1.50:8080"),
        Some("openkwl"),
        Some("OpenKWL.local"),
        Some("openkwl.fritz.box"),
        Some("openkwl.home.arpa:80"),
        Some("[fe80::1]"),
        Some("localhost:8080"),
    ] {
        assert!(local_host(ok, "openkwl"), "{ok:?}");
    }
    for bad in [Some("evil.example"), Some("openkwl.evil.example"), Some("openkwl-x.local"), Some("1.2.3"), Some("helios.local")] {
        assert!(!local_host(bad, "openkwl"), "{bad:?}");
    }
}

#[test]
fn a_guessing_client_does_not_lock_out_the_owner() {
    let mut h = Harness::logged_in();
    let mut login = |client: u32, pw: &str| {
        let body = format!(r#"{{"password":"{pw}"}}"#);
        let req = Request {
            method: Method::Post,
            path: "/api/v1/auth/login",
            authorization: None,
            client,
            host: None,
            json: true,
            body: body.as_bytes(),
        };
        let hashed = h.app.password_work(&req).run([7; openkwl_auth::SALT_LEN]);
        let mut out = [0u8; 2048];
        h.app.handle_hashed(&req, &mut h.rng, &mut out, hashed).status
    };
    for _ in 0..8 {
        login(0xC0A8_0163, "not the password");
    }
    assert_eq!(login(0xC0A8_0163, PASSWORD), 429, "the guesser is locked");
    assert_eq!(login(0xC0A8_0102, PASSWORD), 200, "the owner is not");
}

/// A KWL-KNX answering a poll: its 16 input registers as a function-04 answer.
fn knx_answer(reg0: u16, reg1: u16, reg12: u16) -> std::vec::Vec<u8> {
    let regs = [reg0, reg1, 0, 0, 0x14, 3, 0, 0, 100, 0, 120, 0, reg12, 0, 0, 0];
    let mut f = std::vec![0x14, 0x04, 0x20];
    for r in regs {
        f.extend_from_slice(&r.to_be_bytes());
    }
    let c = openkwl_bus::modbus::crc(&f);
    f.extend_from_slice(&c.to_le_bytes());
    f
}

#[test]
fn knx_module_reads_the_unit_and_sets_the_level() {
    use openkwl_bus::knx::{Followup, Module};
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    let mut m = Module::new();
    let mut out = [0u8; openkwl_bus::modbus::FRAME_MAX];

    // The module asks for block 1: level, Auto, date and time come from the app.
    let (cmd, f) = m.on_answer(&knx_answer(1, 1, 0x80), &h.app.knx_snapshot(), &mut out).unwrap();
    assert_eq!(cmd, None, "the first answer after start-up is not applied");
    let Followup::Write { len, .. } = f else { panic!("{f:?}") };
    let level_byte = out[13 + 1]; // register 3 lo byte = struct byte 0
    assert_eq!(level_byte, 0x80 | h.app.control().level, "{:02x?}", &out[..len]);
    // Register 4 = struct bytes 2 (month, lo) and 3 (day, hi), so on the wire: day, month.
    assert_eq!((out[15], out[16]), (19, 10), "2026-10-19");

    // The module reports Auto (the unit is in Auto anyway), then a KNX switch picks manual
    // level 4: the unit follows like a web UI choice.
    let (cmd, _) = m.on_answer(&knx_answer(0, 0, 0x80), &h.app.knx_snapshot(), &mut out).unwrap();
    assert_eq!(cmd, Some(openkwl_bus::knx::FanCommand::Auto));
    h.app.knx_command(cmd.unwrap());
    let (cmd, _) = m.on_answer(&knx_answer(0, 0, 0x04), &h.app.knx_snapshot(), &mut out).unwrap();
    h.app.knx_command(cmd.expect("a change"));
    let c = h.app.control();
    assert_eq!((c.mode, c.level), (openkwl_api::Mode::Manual, 4));

    // Selecting weekly program "Standard 3" (index 2) from KNX.
    h.app.knx_push(openkwl_bus::knx::Push::Programme(2));
    assert_eq!(h.app.config().schedule.active_profile, 3);
    let s = h.app.knx_snapshot();
    assert_eq!((s.programme, s.auto), (2, false));
}

#[test]
fn a_mode_change_is_saved_within_a_minute() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    let _ = h.app.take_writes();
    h.call(Method::Patch, "/control", Some(json!({ "mode": "manual", "level": 3 })));
    h.tick(30, Some(MONDAY_NOON + 1), &inputs());
    assert_eq!(h.app.take_writes().state, None, "waits for quiet");
    h.tick(31, Some(MONDAY_NOON + 31), &inputs());
    let st = h.app.take_writes().state.expect("saved after 60 s quiet");
    assert_eq!((st.auto_mode, st.manual_level), (false, 3));
    h.tick(120, Some(MONDAY_NOON + 62), &inputs());
    assert_eq!(h.app.take_writes().state, None, "once");
}

#[test]
fn a_date_correction_is_not_midnight() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    h.call(Method::Patch, "/control", Some(json!({ "level": 3 })));
    // The clock jumps a day ahead at noon (SNTP fixing a wrong RTC): manual stays.
    h.tick(1, Some(MONDAY_NOON + 86_400), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Manual);
    // And back again.
    h.tick(1, Some(MONDAY_NOON), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Manual);
}

#[test]
fn a_clock_correction_onto_the_next_day_is_not_midnight() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    h.call(Method::Patch, "/control", Some(json!({ "level": 3 })));
    // Monday 12:00 -> Tuesday 11:00 (+23 h): a later date with an earlier time of day.
    h.tick(1, Some(MONDAY_NOON + 23 * 3600), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Manual);
    // Tuesday 23:00 -> Wednesday 00:30 in one step: no midnight either.
    h.tick(1, Some(MONDAY_NOON + 35 * 3600), &inputs());
    h.tick(1, Some(MONDAY_NOON + 36 * 3600 + 1800), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Manual);
    // A real midnight with a 90 s gap in the ticks (a busy control loop) still counts.
    h.tick(1, Some(MONDAY_NOON + 59 * 3600 + 3570), &inputs());
    h.tick(1, Some(MONDAY_NOON + 60 * 3600 + 60), &inputs());
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Auto);
}

#[test]
fn a_holiday_refuses_level_and_mode_changes() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    let holiday =
        json!({ "enabled": true, "start": "2026-10-01", "end": "2026-12-31", "level": 1, "interval_h": 2, "active_min": 60 });
    assert_eq!(h.call(Method::Put, "/config/holiday", Some(holiday.clone())).0, 200);
    h.tick(1, Some(MONDAY_NOON + 1), &inputs());
    let before = h.app.control();
    assert_eq!((before.mode, before.base), (openkwl_api::Mode::Holiday, openkwl_api::BaseMode::Auto));

    let (s, v) = h.call(Method::Patch, "/control", Some(json!({ "level": 0 })));
    assert_eq!((s, v["field"].as_str()), (422, Some("level")), "{v}");
    assert!(v["message"].as_str().unwrap().contains("holiday"), "{v}");
    let (s, v) = h.call(Method::Patch, "/control", Some(json!({ "mode": "party" })));
    assert_eq!((s, v["field"].as_str()), (422, Some("mode")));
    assert_eq!(h.app.control(), before, "nothing changed behind the holiday");

    // Home Assistant: ignored, but reported as handled so the state is republished at once.
    let t = format!("{}/fan/set", h.app.config().mqtt.base_topic);
    let _ = h.app.ha_state_due(true);
    assert!(h.app.ha_command(&t, b"OFF"));
    assert_eq!(h.app.control(), before);
    // The knx module is refused too.
    h.app.knx_command(openkwl_bus::knx::FanCommand::Manual(0));
    assert_eq!(h.app.control(), before);

    // Ending the holiday works and the weekly program takes over again.
    let mut off = holiday;
    off["enabled"] = json!(false);
    assert_eq!(h.call(Method::Put, "/config/holiday", Some(off)).0, 200);
    let c = h.app.control();
    assert_eq!((c.mode, c.level), (openkwl_api::Mode::Auto, 2));
    assert_eq!(h.call(Method::Patch, "/control", Some(json!({ "level": 0 }))).0, 200);
}

#[test]
fn a_filter_reset_is_saved_promptly() {
    let mut h = Harness::logged_in();
    h.tick(1200, Some(MONDAY_NOON), &inputs());
    let _ = h.app.take_writes();
    assert!(h.app.filter.elapsed_s() > 0);
    assert_eq!(h.call(Method::Post, "/filter/reset", None).0, 204);
    let st = h.app.take_writes().state.expect("saved with the next take_writes");
    assert_eq!(st.filter_elapsed_s, 0);
    h.tick(10, Some(MONDAY_NOON + 3600), &inputs());
    assert_eq!(h.app.take_writes().state, None, "once");
    let t = format!("{}/filter/reset", h.app.config().mqtt.base_topic);
    assert!(h.app.ha_command(&t, b"PRESS"));
    assert_eq!(h.app.take_writes().state.map(|s| s.filter_elapsed_s), Some(0), "also from Home Assistant");
}

#[test]
fn mode_saves_are_capped_per_day() {
    let mut h = Harness::logged_in();
    let mut writes = 0;
    let mut saved_late = false;
    let t = format!("{}/fan/level/set", h.app.config().mqtt.base_topic);
    // A Home Assistant automation switching the level every 15 minutes for a day.
    for k in 0..96u64 {
        assert!(h.app.ha_command(&t, if k % 2 == 0 { b"1" } else { b"2" }));
        for s in 0..900 {
            h.app.tick_second(&inputs(), Some(MONDAY_NOON + k * 900 + s));
            if let Some(st) = h.app.take_writes().state {
                writes += 1;
                assert_eq!(st.manual_level, h.app.control().manual_level, "a save holds the current mode");
                saved_late |= k >= 24;
            }
        }
    }
    // At most 4 periodic saves + MODE_SAVES_PER_DAY (a prompt save restarts the 6 h period);
    // without the cap it would be 96.
    assert!(writes <= 4 + u32::from(MODE_SAVES_PER_DAY), "{writes} writes");
    assert!(saved_late, "after the cap the 6-hourly save still records the mode");
    // A new day: prompt saves again.
    assert!(h.app.ha_command(&t, b"4"));
    h.tick(61, Some(MONDAY_NOON + 86_400), &inputs());
    assert_eq!(h.app.take_writes().state.map(|s| s.manual_level), Some(4));
}

#[test]
fn a_saved_config_clears_the_config_fault() {
    let mut h = Harness::new();
    h.app = App::new(Boot { config_invalid: true, ..boot() });
    h.app.set_connectivity(Connectivity { sd_card: true, ..Default::default() });
    let (_, v) = h.call(Method::Post, "/setup", Some(json!({ "password": PASSWORD })));
    h.token = Some(v["token"].as_str().unwrap().into());
    assert!(h.app.status().faults.contains(&openkwl_api::Fault::ConfigInvalid));
    let b = serde_json::to_value(h.app.config().bypass).unwrap();
    assert_eq!(h.call(Method::Put, "/config/bypass", Some(b)).0, 200);
    assert!(h.app.status().faults.contains(&openkwl_api::Fault::ConfigInvalid), "not before it is written");
    h.tick(15, None, &inputs());
    assert!(h.app.take_writes().config);
    assert!(!h.app.status().faults.contains(&openkwl_api::Fault::ConfigInvalid));
    assert_eq!(h.call(Method::Get, "/system/details", None).1["storage"]["config_valid"], true);
}

#[test]
fn without_saved_state_the_unit_runs_auto() {
    let app = App::new(Boot { state: RuntimeState::default(), ..boot() });
    let c = app.control();
    assert_eq!((c.mode, c.manual_level), (openkwl_api::Mode::Auto, 2));
}

#[test]
fn status_reports_today_and_the_holiday() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    let (_, st) = h.call(Method::Get, "/status", None);
    let today = &st["schedule"]["today"];
    assert_eq!(today[0]["from"], "00:00", "{st}");
    assert_eq!(st["schedule"]["next"], Value::Null, "the default program never changes level");
    assert_eq!(st["holiday"], Value::Null);

    let holiday =
        json!({ "enabled": true, "start": "2026-10-01", "end": "2026-12-31", "level": 1, "interval_h": 2, "active_min": 60 });
    assert_eq!(h.call(Method::Put, "/config/holiday", Some(holiday)).0, 200);
    h.tick(1, Some(MONDAY_NOON + 1), &inputs());
    let (_, st) = h.call(Method::Get, "/status", None);
    assert_eq!(st["holiday"]["until"], "2026-12-31", "{st}");
    assert_eq!(st["control"]["mode"], "holiday");
}

#[test]
fn ending_a_program_returns_to_the_manual_level() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    h.call(Method::Patch, "/control", Some(json!({ "mode": "manual", "level": 1 })));
    let (_, c) = h.call(Method::Patch, "/control", Some(json!({ "mode": "party", "duration_min": 30 })));
    assert_eq!((c["mode"].as_str(), c["base"].as_str(), c["manual_level"].as_u64()), (Some("party"), Some("manual"), Some(1)));
    let (_, c) = h.call(Method::Patch, "/control", Some(json!({ "end_program": true })));
    assert_eq!((c["mode"].as_str(), c["level"].as_u64()), (Some("manual"), Some(1)));
    assert_eq!(h.call(Method::Patch, "/control", Some(json!({ "end_program": true, "mode": "auto" }))).0, 422);
}

#[test]
fn frost_protection_stops_the_supply_fan() {
    let mut h = Harness::logged_in();
    let iced = Inputs {
        outdoor: Reading::Celsius(-8.0),
        supply: Reading::Celsius(3.6), // η ≈ 40 %
        extract: Reading::Celsius(21.0),
        ..inputs()
    };
    let out = h.tick(5, Some(MONDAY_NOON), &iced);
    assert_eq!(out.exhaust_volts, 7.2, "6 V + 1.2 V boost");
    let out = h.tick(15 * 60 + 5, Some(MONDAY_NOON + 5), &iced);
    assert_eq!(out.supply_duty, 0, "defrost: supply fan off");
    let st = h.app.status();
    assert!(st.frost.active && st.frost.defrost);
    assert!(!out.bypass_open);
}

#[test]
fn bypass_opens_in_summer_and_respects_the_season() {
    let mut h = Harness::logged_in();
    let summer =
        Inputs { outdoor: Reading::Celsius(16.0), supply: Reading::Celsius(17.0), extract: Reading::Celsius(25.0), ..inputs() };
    assert!(h.tick(1, Some(MONDAY_NOON), &summer).bypass_open);
    assert!(h.app.status().bypass.moving);
    assert_eq!(h.app.status().heat_recovery.power_w, Some(0.0));

    let mut b = serde_json::to_value(h.app.config().bypass).unwrap();
    b["season"] = json!({ "start": "05-01", "end": "09-30" });
    assert_eq!(h.call(Method::Put, "/config/bypass", Some(b)).0, 200);
    assert!(!h.tick(1, Some(MONDAY_NOON + 1), &summer).bypass_open, "October is outside the season");
}

#[test]
fn sensor_faults_and_heat_recovery() {
    let mut h = Harness::logged_in();
    h.app.config.fans.airflow_m3h = [100, 150, 200, 250];
    // 22.1 °C extract air is above the 22 °C target, so the bypass would open (and recover
    // nothing); keep it closed for this test.
    h.app.config.bypass.enabled = false;
    let broken = Inputs { exhaust: Reading::Open, ..inputs() };
    h.tick(1, Some(MONDAY_NOON), &broken);
    let st = h.app.status();
    assert!(st.faults.contains(&openkwl_api::Fault::ExhaustSensorOpen));
    assert_eq!(st.temperatures.exhaust, None);
    // 0.335 × 150 m³/h × 7 K ≈ 352 W.
    assert!((st.heat_recovery.power_w.unwrap() - 351.75).abs() < 0.5);
}

#[test]
fn settings_are_saved_debounced() {
    let mut h = Harness::logged_in();
    h.app.take_writes();
    let mut b = serde_json::to_value(h.app.config().bypass).unwrap();
    b["target_c"] = json!(24.5);
    assert_eq!(h.call(Method::Put, "/config/bypass", Some(b.clone())).0, 200);
    assert_eq!(h.call(Method::Get, "/config/bypass", None).1["target_c"], 24.5);
    h.tick(5, None, &inputs());
    assert!(!h.app.take_writes().config, "still debouncing");
    h.tick(10, None, &inputs());
    assert!(h.app.take_writes().config);
    assert!(!h.app.take_writes().config, "written once");

    b["target_c"] = json!(99);
    let (s, v) = h.call(Method::Put, "/config/bypass", Some(b));
    assert_eq!((s, v["field"].as_str()), (422, Some("target_c")));
}

#[test]
fn counters_are_saved_periodically_and_before_reboot() {
    let mut h = Harness::logged_in();
    h.tick(6 * 3600, None, &inputs());
    let w = h.app.take_writes();
    assert_eq!(w.state.map(|s| s.operating_s), Some(6 * 3600));
    assert_eq!(h.call(Method::Get, "/status", None).0, 401, "session idle timeout");
    let (_, v) = h.call(Method::Post, "/auth/login", Some(json!({ "password": PASSWORD })));
    h.token = Some(v["token"].as_str().unwrap().into());
    assert_eq!(h.call(Method::Post, "/system/reboot", None).0, 202);
    assert!(h.app.take_writes().state.is_some(), "saved before restart");
    assert!(h.app.take_actions().reboot);
}

#[test]
fn mqtt_password_goes_to_secrets_only() {
    let mut h = Harness::logged_in();
    let mut m = serde_json::to_value(h.app.config().mqtt.clone()).unwrap();
    m.as_object_mut().unwrap().remove("password_set");
    m["enabled"] = json!(true);
    m["host"] = json!("broker.lan");
    m["password"] = json!("s3cret#pw");
    let (s, v) = h.call(Method::Put, "/config/mqtt", Some(m.clone()));
    assert_eq!((s, v["password_set"].as_bool()), (200, Some(true)));
    assert!(!v.to_string().contains("s3cret"));
    assert_eq!(h.app.secrets().mqtt_password, "s3cret#pw");
    let w = h.app.take_writes();
    assert!(w.secrets);
    assert!(h.app.take_actions().mqtt_changed);

    // Same broker without a password: the stored one stays.
    m.as_object_mut().unwrap().remove("password");
    m["username"] = json!("ha");
    assert_eq!(h.call(Method::Put, "/config/mqtt", Some(m.clone())).0, 200);
    assert_eq!(h.app.secrets().mqtt_password, "s3cret#pw");
    // Another broker: only with the password typed again.
    m["host"] = json!("evil.example");
    let (s, v) = h.call(Method::Put, "/config/mqtt", Some(m.clone()));
    assert_eq!((s, v["field"].as_str()), (422, Some("password")));
    assert_eq!(h.app.config().mqtt.host, "broker.lan");
    // An imported file with another broker drops the stored password.
    let text = std::str::from_utf8(&h.raw(Method::Get, "/config/export", None).1.into_bytes())
        .unwrap()
        .replace("broker.lan", "other.lan");
    assert_eq!(h.raw(Method::Put, "/config/import", Some(text)).0, 204);
    assert!(h.app.secrets().mqtt_password.is_empty());
}

#[test]
fn export_and_import_round_trip() {
    let mut h = Harness::logged_in();
    let (s, ini) = h.raw(Method::Get, "/config/export", None);
    assert_eq!(s, 200);
    assert!(ini.contains("[profile5]"));
    let edited = ini.replace("target_c = 22.0", "target_c = 23.5").replace("serial = ", "serial = 999");
    let (s, _) = h.raw(Method::Put, "/config/import", Some(edited));
    assert_eq!(s, 204);
    assert_eq!(h.app.config().bypass.target_c, 23.5);
    assert_eq!(h.app.config().device.identity.serial, "", "device section is read-only");
    let (s, text) = h.raw(Method::Put, "/config/import", Some("[fans]\nsupply_volts = 1, 2\n".into()));
    assert_eq!(s, 422);
    assert!(text.contains("line 2"), "{text}");
}

#[test]
fn system_info_and_set_time() {
    let mut h = Harness::logged_in();
    h.app.config.device.mac = "80:4B:20:12:34:56".try_into().unwrap();
    h.app.config.device.identity.model = "KWL EC 300W R".try_into().unwrap();
    let (s, v) = h.call(Method::Get, "/system", None);
    assert_eq!((s, v["mac"].as_str(), v["device"]["model"].as_str()), (200, Some("80:4b:20:12:34:56"), Some("KWL EC 300W R")));
    assert_eq!(h.call(Method::Post, "/system/time", Some(json!({ "unix_s": MONDAY_NOON }))).0, 204);
    assert_eq!(h.app.take_actions().set_time_unix, Some(MONDAY_NOON));
    assert_eq!(h.app.local_time().map(|t| t.date), clock::Date::new(2026, 10, 19));
    let (_, log) = h.raw(Method::Get, "/log", None);
    assert!(log.contains("clock set manually"), "{log}");
}

#[test]
fn stay_logged_in() {
    let mut h = Harness::logged_in();
    h.token = None;
    let (s, v) = h.call(Method::Post, "/auth/login", Some(json!({ "password": PASSWORD, "remember": true })));
    assert_eq!((s, v["idle_timeout_s"].as_u64()), (200, Some(REMEMBER_IDLE_S)));
    let remembered = String::from(v["token"].as_str().unwrap());
    let (_, v) = h.call(Method::Post, "/auth/login", Some(json!({ "password": PASSWORD })));
    let normal = String::from(v["token"].as_str().unwrap());

    h.tick(2 * 3600, None, &inputs());
    h.token = Some(normal);
    assert_eq!(h.call(Method::Get, "/status", None).0, 401, "normal session idle after 30 min");
    h.token = Some(remembered);
    assert_eq!(h.call(Method::Get, "/status", None).0, 200, "remembered session still valid");

    let change = json!({ "current_password": PASSWORD, "new_password": "another long one" });
    assert_eq!(h.call(Method::Post, "/auth/password", Some(change)).0, 204);
    assert_eq!(h.call(Method::Get, "/status", None).0, 401, "password change ends remembered sessions too");
}

#[test]
fn home_assistant_commands() {
    let mut h = Harness::logged_in();
    h.tick(1, Some(MONDAY_NOON), &inputs());
    let base = String::from(h.app.config().mqtt.base_topic.as_str());
    let t = |s: &str| format!("{base}/{s}");

    assert!(h.app.ha_command(&t("fan/level/set"), b"4"));
    assert_eq!((h.app.control().mode, h.app.control().level), (openkwl_api::Mode::Manual, 4));
    h.tick(1, Some(MONDAY_NOON + 1), &inputs());
    assert!(h.app.ha_command(&t("fan/set"), b"OFF"));
    assert_eq!(h.app.control().level, 0);
    assert!(h.app.ha_command(&t("fan/set"), b"ON"));
    assert_eq!(h.app.control().level, 4, "ON resumes the last running level");
    assert!(h.app.ha_command(&t("fan/preset/set"), b"auto"));
    assert_eq!(h.app.control().mode, openkwl_api::Mode::Auto);

    assert!(h.app.ha_command(&t("bypass/target/set"), b"24.5"));
    assert_eq!(h.app.config().bypass.target_c, 24.5);
    assert!(!h.app.ha_command(&t("bypass/target/set"), b"99"), "validated like the REST API");
    assert!(h.app.ha_command(&t("bypass/enabled/set"), b"OFF"));
    assert!(!h.app.config().bypass.enabled);
    assert!(!h.app.ha_command("other/fan/set", b"ON"));
    assert!(h.app.ha_command(&t("system/reboot"), b"PRESS"));
    assert!(h.app.take_actions().reboot);

    let state = h.app.ha_state();
    assert_eq!((state.bypass_target, state.bypass_enabled), (24.5, openkwl_ha::OnOff::Off));
}

#[test]
fn system_details() {
    let mut h = Harness::logged_in();
    h.app.set_connectivity(Connectivity {
        sd_card: true,
        network: openkwl_api::NetworkDetails { ntp_server: Some("192.168.1.1".try_into().unwrap()), ..Default::default() },
        ..Default::default()
    });
    let _ = h.call(Method::Get, "/nope", None);
    let (s, v) = h.call(Method::Get, "/system/details", None);
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["network"]["ntp_server"], "192.168.1.1");
    assert_eq!(v["time"]["source"], "none");
    assert_eq!(v["counters"]["sessions"], 1);
    assert_eq!(v["counters"]["http_errors"], 1);
    assert_eq!(v["storage"]["present"], true);

    h.tick(1, Some(1_792_186_862), &inputs());
    let (_, v) = h.call(Method::Get, "/system/details", None);
    assert_eq!(v["time"]["source"], "rtc");
    let (s, _) = h.call(Method::Post, "/system/time", Some(json!({ "unix_s": 1_792_186_862u64 })));
    assert_eq!(s, 204);
    let (_, v) = h.call(Method::Get, "/system/details", None);
    assert_eq!(v["time"]["source"], "manual");
}

#[test]
fn home_assistant_gets_changes_promptly() {
    let mut h = Harness::logged_in();
    let i = inputs();
    h.tick(1, Some(1_792_186_862), &i);
    assert!(h.app.ha_state_due(false).is_some(), "first state goes out at once");
    assert!(h.app.ha_state_due(false).is_none(), "nothing changed");

    // A level change from the web UI reaches HA within the same second.
    let (s, _) = h.call(Method::Patch, "/control", Some(json!({ "level": 3 })));
    assert_eq!(s, 200);
    let st = h.app.ha_state_due(false).expect("switching change is published at once");
    assert_eq!(st.level, 3);

    // Unchanged: only the heartbeat.
    h.tick(HA_HEARTBEAT_S - 2, Some(1_792_186_863), &i);
    let _ = h.app.ha_state_due(false); // fan spin-up may have moved measurements
    h.tick(1, Some(1_792_186_863 + HA_HEARTBEAT_S), &i);
    assert!(h.app.ha_state_due(false).is_none());
    h.tick(HA_HEARTBEAT_S, Some(1_792_186_864 + HA_HEARTBEAT_S), &i);
    assert!(h.app.ha_state_due(false).is_some(), "heartbeat");
    assert!(h.app.ha_state_due(true).is_some(), "forced");
}

#[test]
fn home_assistant_measurements_are_rate_limited() {
    let mut h = Harness::logged_in();
    // Measured air flows: a fan-speed change then moves only the diagnostic values.
    h.app.config.fans.airflow_m3h = [100, 150, 200, 250];
    let mut i = inputs();
    let t0 = 1_792_186_862;
    h.tick(1, Some(t0), &i);
    assert!(h.app.ha_state_due(false).is_some());

    // A temperature move waits for the measurement interval.
    i.outdoor = Reading::Celsius(13.8);
    h.tick(HA_MEASUREMENT_MIN_S - 2, Some(t0 + 1), &i);
    assert!(h.app.ha_state_due(false).is_none(), "too soon");
    h.tick(2, Some(t0 + HA_MEASUREMENT_MIN_S), &i);
    assert!(h.app.ha_state_due(false).is_some(), "after the interval");

    // A fan-speed move alone waits for the longer diagnostic interval.
    i.supply_rpm += 200;
    h.tick(HA_MEASUREMENT_MIN_S, Some(t0 + 2 * HA_MEASUREMENT_MIN_S), &i);
    assert!(h.app.ha_state_due(false).is_none(), "diagnostics wait longer");
    h.tick(HA_DIAGNOSTIC_MIN_S - HA_MEASUREMENT_MIN_S, Some(t0 + HA_DIAGNOSTIC_MIN_S + HA_MEASUREMENT_MIN_S), &i);
    assert!(h.app.ha_state_due(false).is_some());
}

#[test]
fn history_today_is_a_day_record() {
    let mut h = Harness::logged_in();
    let get = |h: &mut Harness| {
        let auth = h.token.as_ref().map(|t| format!("Bearer {t}"));
        let req = Request {
            method: Method::Get,
            path: openkwl_api::paths::HISTORY_TODAY,
            authorization: auth.as_deref(),
            client: 0,
            host: None,
            json: false,
            body: b"",
        };
        let mut out = [0u8; 4096];
        let r = h.app.handle(&req, &mut h.rng, &mut out);
        (r.status, r.content_type, out[..r.len].to_vec())
    };
    assert_eq!(get(&mut h).0, 404, "no clock, no record");
    h.tick(600, Some(MONDAY_NOON), &inputs());
    let (status, content_type, body) = get(&mut h);
    assert_eq!((status, content_type, body.len()), (200, crate::http::BINARY, crate::history::RECORD_LEN));
    let day = (MONDAY_NOON / 86_400) as u16;
    assert!(crate::history::valid(&body, day), "a local day (the test zone keeps noon on that day)");
    assert_eq!(u32::from_le_bytes(body[12..16].try_into().unwrap()), 600);
}
