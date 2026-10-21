//! `CONFIG.INI` and `SECRETS.INI` format.
//!
//! ```ini
//! # comment
//! [section]
//! key = value   # inline comment (CONFIG.INI only)
//! ```
//!
//! - Reading starts from a base config (normally [`Config::default`]); keys that are missing
//!   keep the base value, so files from older firmware still load.
//! - Unknown sections and keys are reported as warnings, not errors.
//! - A malformed value fails with its line number. After parsing, every section is checked
//!   with the same `Validate` rules as the REST API.
//! - An empty value means "none" for optional fields (`address =`, `season =`).
//! - Inline comments need a space before `#`. `SECRETS.INI` has no inline comments, so
//!   passwords may contain `#`.

use core::fmt::{self, Write};
use core::str::FromStr;

use openkwl_api::{
    ActuatorType, FieldError, Season, Slot, Validate,
    time::{Date, MonthDay, TimeOfDay},
};
use openkwl_core::schedule::MAX_SLOTS;

use crate::{Config, Secrets};

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Line is neither a section, a comment nor `key = value`.
    Syntax,
    /// Value has the wrong format or does not fit.
    BadValue,
    /// Parsed, but fails validation: `(section, field error)`.
    Invalid(&'static str, FieldError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// 1-based line; 0 for validation errors.
    pub line: u32,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    UnknownSection,
    UnknownKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warning {
    pub line: u32,
    pub kind: WarningKind,
}

/// Warnings of a successful read (the first 8; `more` counts the rest).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub warnings: heapless::Vec<Warning, 8>,
    pub more: u16,
}

impl Report {
    fn warn(&mut self, line: u32, kind: WarningKind) {
        if self.warnings.push(Warning { line, kind }).is_err() {
            self.more = self.more.saturating_add(1);
        }
    }
}

/// First position of the two ASCII bytes `a b`. Same as `str::find("ab")`, without pulling the
/// generic substring searcher (about 1 KB) into the firmware.
fn find_pair(s: &str, a: u8, b: u8) -> Option<usize> {
    s.as_bytes().windows(2).position(|w| w == [a, b])
}

/// Splits into `(line_no, content)` pairs, skipping blanks and comments; handles CRLF and a
/// UTF-8 BOM.
fn lines(text: &str, inline_comments: bool) -> impl Iterator<Item = (u32, &str)> {
    text.strip_prefix('\u{feff}').unwrap_or(text).lines().enumerate().filter_map(move |(i, l)| {
        let mut l = l.trim_end_matches('\r');
        if inline_comments && let Some(p) = find_pair(l, b' ', b'#').or_else(|| find_pair(l, b'\t', b'#')) {
            l = &l[..p];
        }
        let l = l.trim();
        (!l.is_empty() && !l.starts_with('#') && !l.starts_with(';')).then_some((i as u32 + 1, l))
    })
}

enum Line<'a> {
    Section(&'a str),
    Pair(&'a str, &'a str),
}

fn classify(l: &str) -> Option<Line<'_>> {
    if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Some(Line::Section(name.trim()));
    }
    let (k, v) = l.split_once('=')?;
    Some(Line::Pair(k.trim(), v.trim()))
}

// ---------------------------------------------------------------------------------------
// Value parsers. `Err(())` becomes `ErrorKind::BadValue` with the line number.

type V<T> = Result<T, ()>;

fn boolean(v: &str) -> V<bool> {
    match v {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => Err(()),
    }
}

/// Numbers in the INI file. Floats go through `openkwl_core::num::parse_f32` instead of
/// `core`'s parser, which would cost the firmware ~13 KB of flash.
trait Num: Sized {
    fn parse_num(v: &str) -> Option<Self>;
}

macro_rules! int_num {
    ($($t:ty),*) => {$(
        impl Num for $t {
            fn parse_num(v: &str) -> Option<Self> {
                <$t as FromStr>::from_str(v).ok()
            }
        }
    )*};
}
int_num!(u8, u16, u32, u64, i8, i16, i32);

impl Num for f32 {
    fn parse_num(v: &str) -> Option<Self> {
        serde_json_core::parse_float(v).map(|f| f as f32)
    }
}

fn num<T: Num>(v: &str) -> V<T> {
    T::parse_num(v).ok_or(())
}

fn string<const N: usize>(v: &str) -> V<heapless::String<N>> {
    v.try_into().map_err(|_| ())
}

fn opt_string<const N: usize>(v: &str) -> V<Option<heapless::String<N>>> {
    if v.is_empty() { Ok(None) } else { string(v).map(Some) }
}

fn opt_num<T: Num>(v: &str) -> V<Option<T>> {
    if v.is_empty() { Ok(None) } else { num(v).map(Some) }
}

fn list<T: Num + Copy + Default, const N: usize>(v: &str) -> V<[T; N]> {
    let mut out = [T::default(); N];
    let mut it = v.split(',');
    for slot in out.iter_mut() {
        *slot = num(it.next().ok_or(())?.trim())?;
    }
    if it.next().is_some() { Err(()) } else { Ok(out) }
}

fn date(v: &str) -> V<Date> {
    Date::try_from(string::<10>(v)?).map_err(|_| ())
}

fn month_day(v: &str) -> V<MonthDay> {
    MonthDay::try_from(string::<5>(v.trim())?).map_err(|_| ())
}

fn season(v: &str) -> V<Option<Season>> {
    if v.is_empty() {
        return Ok(None);
    }
    let p = find_pair(v, b'.', b'.').ok_or(())?;
    let (a, b) = (&v[..p], &v[p + 2..]);
    Ok(Some(Season { start: month_day(a)?, end: month_day(b)? }))
}

fn actuator(v: &str) -> V<ActuatorType> {
    match v {
        "analog" => Ok(ActuatorType::Analog),
        "motor" => Ok(ActuatorType::Motor),
        "motor_reversed" => Ok(ActuatorType::MotorReversed),
        _ => Err(()),
    }
}

/// `00:00=1, 06:30=3`; empty = no slots.
fn day(v: &str) -> V<heapless::Vec<Slot, MAX_SLOTS>> {
    let mut out = heapless::Vec::new();
    if v.is_empty() {
        return Ok(out);
    }
    for item in v.split(',') {
        let (t, l) = item.trim().split_once('=').ok_or(())?;
        let from = TimeOfDay::try_from(string::<5>(t.trim())?).map_err(|_| ())?;
        out.push(Slot { from, level: num(l.trim())? }).map_err(|_| ())?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Reading

/// Applies one `key = value`; `Ok(false)` for an unknown key.
fn apply(c: &mut Config, section: &str, key: &str, v: &str) -> V<bool> {
    match (section, key) {
        ("network", "dhcp") => c.network.dhcp = boolean(v)?,
        ("network", "hostname") => c.network.hostname = string(v)?,
        ("network", "address") => c.network.address = opt_string(v)?,
        ("network", "netmask") => c.network.netmask = opt_string(v)?,
        ("network", "gateway") => c.network.gateway = opt_string(v)?,
        ("network", "dns") => c.network.dns = opt_string(v)?,

        ("time", "ntp") => c.time.ntp_enabled = boolean(v)?,
        ("time", "ntp_server") => c.time.ntp_server = string(v)?,
        ("time", "utc_offset_min") => c.time.utc_offset_min = num(v)?,
        ("time", "eu_dst") => c.time.eu_dst = boolean(v)?,

        ("mqtt", "enabled") => c.mqtt.enabled = boolean(v)?,
        ("mqtt", "host") => c.mqtt.host = string(v)?,
        ("mqtt", "port") => c.mqtt.port = num(v)?,
        ("mqtt", "username") => c.mqtt.username = string(v)?,
        ("mqtt", "client_id") => c.mqtt.client_id = string(v)?,
        ("mqtt", "base_topic") => c.mqtt.base_topic = string(v)?,
        ("mqtt", "discovery") => c.mqtt.discovery = boolean(v)?,
        ("mqtt", "discovery_prefix") => c.mqtt.discovery_prefix = string(v)?,

        ("fans", "supply_volts") => c.fans.supply_volts = list(v)?,
        ("fans", "extract_volts") => c.fans.extract_volts = list(v)?,
        ("fans", "airflow_m3h") => c.fans.airflow_m3h = list(v)?,
        ("fans", "m3h_per_krpm") => c.fans.m3h_per_krpm = num(v)?,

        ("programs", "party_level") => c.programs.party_level = num(v)?,
        ("programs", "party_min") => c.programs.party_min = num(v)?,
        ("programs", "quiet_level") => c.programs.quiet_level = num(v)?,
        ("programs", "quiet_min") => c.programs.quiet_min = num(v)?,

        ("bypass", "enabled") => c.bypass.enabled = boolean(v)?,
        ("bypass", "target_c") => c.bypass.target_c = num(v)?,
        ("bypass", "min_outdoor_c") => c.bypass.min_outdoor_c = num(v)?,
        ("bypass", "close_if_outdoor_warmer") => c.bypass.close_if_outdoor_warmer = boolean(v)?,
        ("bypass", "actuator") => c.bypass.actuator = actuator(v)?,
        ("bypass", "run_time_s") => c.bypass.run_time_s = num(v)?,
        ("bypass", "season") => c.bypass.season = season(v)?,
        ("bypass", "supply_offset_k") => c.bypass.supply_offset_k = num(v)?,

        ("frost", "arm_below_c") => c.frost.arm_below_c = num(v)?,
        ("frost", "entry_pct") => c.frost.entry_pct = num(v)?,
        ("frost", "hold_pct") => c.frost.hold_pct = num(v)?,
        ("frost", "exit_pct") => c.frost.exit_pct = num(v)?,
        ("frost", "boost_delay_s") => c.frost.boost_delay_s = num(v)?,
        ("frost", "defrost_s") => c.frost.defrost_s = num(v)?,
        ("frost", "test_run_s") => c.frost.test_run_s = num(v)?,
        ("frost", "exhaust_boost_v") => c.frost.exhaust_boost_v = num(v)?,

        ("holiday", "enabled") => c.holiday.enabled = boolean(v)?,
        ("holiday", "start") => c.holiday.start = date(v)?,
        ("holiday", "end") => c.holiday.end = date(v)?,
        ("holiday", "level") => c.holiday.level = num(v)?,
        ("holiday", "interval_h") => c.holiday.interval_h = num(v)?,
        ("holiday", "active_min") => c.holiday.active_min = num(v)?,

        ("filter", "interval_months") => c.filter.interval_months = num(v)?,

        ("schedule", "active_profile") => c.schedule.active_profile = num(v)?,
        ("schedule", "auto_at_midnight") => c.schedule.auto_at_midnight = boolean(v)?,
        ("schedule", "auto_return_min") => c.schedule.auto_return_min = opt_num(v)?,
        ("schedule", "fallback_level") => c.schedule.fallback_level = num(v)?,

        ("device", "model") => c.device.identity.model = string(v)?,
        ("device", "article") => c.device.identity.article = string(v)?,
        ("device", "serial") => c.device.identity.serial = string(v)?,
        ("device", "production_code") => c.device.identity.production_code = string(v)?,
        ("device", "security_code") => c.device.identity.security_code = string(v)?,
        ("device", "mac") => c.device.mac = string(v)?,

        (p, d) if profile_index(p).is_some() && DAYS.contains(&d) => {
            let (pi, di) = (profile_index(p).unwrap(), DAYS.iter().position(|x| *x == d).unwrap());
            c.profiles[pi].days[di] = day(v)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `profile1` … `profile5` → 0 … 4.
fn profile_index(section: &str) -> Option<usize> {
    let n: usize = section.strip_prefix("profile")?.parse().ok()?;
    (1..=openkwl_core::schedule::PROFILES).contains(&n).then(|| n - 1)
}

const SECTIONS: [&str; 11] =
    ["network", "time", "mqtt", "fans", "programs", "bypass", "frost", "holiday", "filter", "schedule", "device"];

fn known_section(s: &str) -> bool {
    profile_index(s).is_some() || SECTIONS.contains(&s)
}

/// Checks every section with the REST API rules.
pub fn validate(c: &Config) -> Result<(), Error> {
    let invalid = |section| move |e| Error { line: 0, kind: ErrorKind::Invalid(section, e) };
    c.network.validate().map_err(invalid("network"))?;
    c.time.validate().map_err(invalid("time"))?;
    let mqtt = openkwl_api::MqttConfigUpdate {
        enabled: c.mqtt.enabled,
        host: c.mqtt.host.clone(),
        port: c.mqtt.port,
        username: c.mqtt.username.clone(),
        password: None,
        client_id: c.mqtt.client_id.clone(),
        base_topic: c.mqtt.base_topic.clone(),
        discovery: c.mqtt.discovery,
        discovery_prefix: c.mqtt.discovery_prefix.clone(),
    };
    mqtt.validate().map_err(invalid("mqtt"))?;
    c.fans.validate().map_err(invalid("fans"))?;
    c.programs.validate().map_err(invalid("programs"))?;
    c.bypass.validate().map_err(invalid("bypass"))?;
    c.frost.validate().map_err(invalid("frost"))?;
    c.holiday.validate().map_err(invalid("holiday"))?;
    c.filter.validate().map_err(invalid("filter"))?;
    c.schedule.validate().map_err(invalid("schedule"))?;
    const PROFILE_SECTIONS: [&str; openkwl_core::schedule::PROFILES] = ["profile1", "profile2", "profile3", "profile4", "profile5"];
    for (p, name) in c.profiles.iter().zip(PROFILE_SECTIONS) {
        p.validate().map_err(invalid(name))?;
    }
    Ok(())
}

/// Reads `CONFIG.INI` on top of `base` and validates the result.
pub fn read_config(text: &str, base: Config) -> Result<(Config, Report), Error> {
    let mut c = base;
    let mut report = Report::default();
    let mut section: Option<&str> = None;
    for (line, l) in lines(text, true) {
        match classify(l).ok_or(Error { line, kind: ErrorKind::Syntax })? {
            Line::Section(s) => {
                if known_section(s) {
                    section = Some(s);
                } else {
                    report.warn(line, WarningKind::UnknownSection);
                    section = None;
                }
            }
            Line::Pair(k, v) => match section {
                None => report.warn(line, WarningKind::UnknownKey),
                Some(s) => {
                    let known = apply(&mut c, s, k, v).map_err(|()| Error { line, kind: ErrorKind::BadValue })?;
                    if !known {
                        report.warn(line, WarningKind::UnknownKey);
                    }
                }
            },
        }
    }
    validate(&c)?;
    Ok((c, report))
}

/// Reads `SECRETS.INI` (`[mqtt] password = …`; the rest of the line is the password).
pub fn read_secrets(text: &str) -> Result<Secrets, Error> {
    let mut s = Secrets::default();
    let mut section = "";
    for (line, l) in lines(text, false) {
        match classify(l).ok_or(Error { line, kind: ErrorKind::Syntax })? {
            Line::Section(name) => section = name,
            Line::Pair("password", v) if section == "mqtt" => {
                s.mqtt_password = string(v).map_err(|()| Error { line, kind: ErrorKind::BadValue })?;
            }
            Line::Pair(..) => {}
        }
    }
    Ok(s)
}

// ---------------------------------------------------------------------------------------
// Writing

struct Opt<'a, T>(&'a Option<T>);

impl<T: fmt::Display> fmt::Display for Opt<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => v.fmt(f),
            None => Ok(()),
        }
    }
}

/// Floats are written in the shortest round-trip form (e.g. `5.0`, `-3.1`, as `ryu` writes
/// them) by serde-json-core's small float writer (`vendor/README.md`) instead of `core::fmt`,
/// whose float code would add ~12 KB to the firmware. Non-finite values (never valid in the
/// config) are written `NaN`, `inf` and `-inf`.
struct F(f32);

impl fmt::Display for F {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        if v.is_nan() {
            return f.write_str("NaN");
        }
        if v.is_infinite() {
            return f.write_str(if v > 0.0 { "inf" } else { "-inf" });
        }
        let mut buf = [0; 24];
        let len = serde_json_core::to_slice(&v, &mut buf).map_err(|_| fmt::Error)?;
        f.write_str(core::str::from_utf8(&buf[..len]).map_err(|_| fmt::Error)?)
    }
}

fn join<T: fmt::Display>(w: &mut impl Write, items: &[T]) -> fmt::Result {
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            w.write_str(", ")?;
        }
        write!(w, "{x}")?;
    }
    Ok(())
}

fn md(m: &MonthDay) -> heapless::String<5> {
    (*m).into()
}

/// Writes the canonical `CONFIG.INI`.
pub fn write_config(c: &Config, w: &mut impl Write) -> fmt::Result {
    writeln!(w, "# openKWL settings, read at boot. Reference: docs/CONFIG_INI.md")?;

    let n = &c.network;
    writeln!(w, "\n[network]")?;
    writeln!(w, "dhcp = {}\nhostname = {}", n.dhcp, n.hostname)?;
    writeln!(w, "address = {}\nnetmask = {}", Opt(&n.address), Opt(&n.netmask))?;
    writeln!(w, "gateway = {}\ndns = {}", Opt(&n.gateway), Opt(&n.dns))?;

    let t = &c.time;
    writeln!(w, "\n[time]\n# ntp_server empty: the router, else pool.ntp.org")?;
    writeln!(w, "ntp = {}\nntp_server = {}", t.ntp_enabled, t.ntp_server)?;
    writeln!(w, "utc_offset_min = {}\neu_dst = {}", t.utc_offset_min, t.eu_dst)?;

    let m = &c.mqtt;
    writeln!(w, "\n[mqtt]")?;
    writeln!(w, "enabled = {}\nhost = {}\nport = {}\nusername = {}", m.enabled, m.host, m.port, m.username)?;
    writeln!(w, "client_id = {}\nbase_topic = {}", m.client_id, m.base_topic)?;
    writeln!(w, "discovery = {}\ndiscovery_prefix = {}", m.discovery, m.discovery_prefix)?;

    writeln!(w, "\n[fans]\n# Levels 1-4 in V (off below 1.65). airflow_m3h: measured, 0 = from m3h_per_krpm")?;
    write!(w, "supply_volts = ")?;
    join(w, &c.fans.supply_volts.map(F))?;
    write!(w, "\nextract_volts = ")?;
    join(w, &c.fans.extract_volts.map(F))?;
    write!(w, "\nairflow_m3h = ")?;
    join(w, &c.fans.airflow_m3h)?;
    writeln!(w, "\nm3h_per_krpm = {}", c.fans.m3h_per_krpm)?;

    let p = &c.programs;
    writeln!(w, "\n[programs]")?;
    writeln!(w, "party_level = {}\nparty_min = {}", p.party_level, p.party_min)?;
    writeln!(w, "quiet_level = {}\nquiet_min = {}", p.quiet_level, p.quiet_min)?;

    let b = &c.bypass;
    writeln!(w, "\n[bypass]\n# Cooling: remove the rubber plug first.")?;
    writeln!(w, "enabled = {}\ntarget_c = {}\nmin_outdoor_c = {}", b.enabled, F(b.target_c), F(b.min_outdoor_c))?;
    writeln!(w, "close_if_outdoor_warmer = {}", b.close_if_outdoor_warmer)?;
    let act = match b.actuator {
        ActuatorType::Analog => "analog",
        ActuatorType::Motor => "motor",
        ActuatorType::MotorReversed => "motor_reversed",
    };
    writeln!(w, "actuator = {act}\nrun_time_s = {}", b.run_time_s)?;
    match &b.season {
        Some(s) => writeln!(w, "season = {}..{}", md(&s.start), md(&s.end))?,
        None => writeln!(w, "season =")?,
    }
    writeln!(w, "supply_offset_k = {}", F(b.supply_offset_k))?;

    let f = &c.frost;
    writeln!(w, "\n[frost]")?;
    writeln!(
        w,
        "arm_below_c = {}\nentry_pct = {}\nhold_pct = {}\nexit_pct = {}",
        F(f.arm_below_c),
        F(f.entry_pct),
        F(f.hold_pct),
        F(f.exit_pct)
    )?;
    writeln!(w, "boost_delay_s = {}\ndefrost_s = {}\ntest_run_s = {}", f.boost_delay_s, f.defrost_s, f.test_run_s)?;
    writeln!(w, "exhaust_boost_v = {}", F(f.exhaust_boost_v))?;

    let h = &c.holiday;
    let d = |x: Date| -> heapless::String<10> { x.into() };
    writeln!(w, "\n[holiday]")?;
    writeln!(w, "enabled = {}\nstart = {}\nend = {}", h.enabled, d(h.start), d(h.end))?;
    writeln!(w, "level = {}\ninterval_h = {}\nactive_min = {}", h.level, h.interval_h, h.active_min)?;

    writeln!(w, "\n[filter]")?;
    writeln!(w, "interval_months = {}", c.filter.interval_months)?;

    let s = &c.schedule;
    writeln!(w, "\n[schedule]")?;
    writeln!(w, "active_profile = {}\nauto_at_midnight = {}", s.active_profile, s.auto_at_midnight)?;
    writeln!(w, "auto_return_min = {}\nfallback_level = {}", Opt(&s.auto_return_min), s.fallback_level)?;

    for (i, profile) in c.profiles.iter().enumerate() {
        writeln!(w, "\n[profile{}]", i + 1)?;
        for (name, slots) in DAYS.iter().zip(&profile.days) {
            write!(w, "{name} =")?;
            for (j, slot) in slots.iter().enumerate() {
                let from: heapless::String<5> = slot.from.into();
                write!(w, "{}{from}={}", if j == 0 { " " } else { ", " }, slot.level)?;
            }
            writeln!(w)?;
        }
    }

    let id = &c.device.identity;
    writeln!(w, "\n[device]")?;
    writeln!(w, "model = {}\narticle = {}\nserial = {}", id.model, id.article, id.serial)?;
    writeln!(w, "production_code = {}\nsecurity_code = {}\nmac = {}", id.production_code, id.security_code, c.device.mac)
}

pub fn write_secrets(s: &Secrets, w: &mut impl Write) -> fmt::Result {
    writeln!(w, "# openKWL secrets\n[mqtt]\npassword = {}", s.mqtt_password)
}

#[cfg(test)]
mod tests;
