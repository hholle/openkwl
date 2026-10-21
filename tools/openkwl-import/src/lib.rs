//! Converts the parameters of a unit running the stock easyControls firmware into a
//! openKWL `CONFIG.INI` and initial `STATE.BIN` counters.
//!
//! The stock web server returns every parameter without a login when asked with
//! `POST /` and body `xml=/data/werte<N>.xml` (a capture is in `testdata/`). The
//! weekly program is in `werte9<profile><weekday>.xml`, profile 0–4, weekday 1–7 (Monday
//! first). [`Source`] abstracts over the live unit and a directory of captured files.
//!
//! Every interpretation that is not certain is reported as a [`Note`].

use std::collections::BTreeMap;

use openkwl_api::{
    Season, Slot,
    time::{Date, TimeOfDay},
};
use openkwl_config::{Config, state::RuntimeState};
use openkwl_core::{clock, cwp, filter::FilterCounter, schedule::PROFILES};

/// Pages with settings.
pub const PAGES: [u8; 10] = [1, 3, 4, 6, 7, 9, 10, 11, 12, 13];

/// A file name like `werte12.xml`.
pub fn page_file(page: u8) -> String {
    format!("werte{page}.xml")
}

/// Weekly-program file for stock profile index `profile` (0–4) and `weekday` (1 = Monday).
pub fn profile_file(profile: usize, weekday: usize) -> String {
    format!("werte9{profile}{weekday}.xml")
}

/// Where stock XML comes from.
pub trait Source {
    /// `Ok(None)` when the file does not exist.
    fn fetch(&mut self, file: &str) -> Result<Option<String>, String>;
}

/// Captured files in a directory.
pub struct DirSource(pub std::path::PathBuf);

impl Source for DirSource {
    fn fetch(&mut self, file: &str) -> Result<Option<String>, String> {
        match std::fs::read(self.0.join(file)) {
            Ok(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{file}: {e}")),
        }
    }
}

/// Decodes the `%XX` escapes the stock server uses in some values (`KWL%20FTF%20AD1`) and
/// the basic XML entities.
fn decode_value(v: &str) -> String {
    let v = v
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `<ID>v00012</ID><VA>5.0</VA>` pairs of one stock XML document.
pub fn parse_values(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<ID>") {
        rest = &rest[start + 4..];
        let Some(end) = rest.find("</ID>") else { break };
        let id = rest[..end].trim().to_owned();
        rest = &rest[end + 5..];
        let next_id = rest.find("<ID>").unwrap_or(rest.len());
        let Some(va) = rest[..next_id].find("<VA>") else {
            continue;
        };
        let after = &rest[va + 4..];
        let Some(va_end) = after.find("</VA>") else {
            break;
        };
        out.push((id, decode_value(after[..va_end].trim())));
        rest = &after[va_end + 5..];
    }
    out
}

/// One stock day: `(from, to, level)` as stored, unused slots dropped.
pub type StockDay = Vec<(String, String, u8)>;

pub fn parse_day(xml: &str) -> StockDay {
    let v: BTreeMap<String, String> = parse_values(xml).into_iter().collect();
    (0..10)
        .filter_map(|i| {
            let from = v.get(&format!("v0094{i}"))?;
            let to = v.get(&format!("v0095{i}"))?;
            let level = v.get(&format!("v0093{i}"))?.parse().ok()?;
            (i == 0 || from != to).then(|| (from.clone(), to.clone(), level))
        })
        .collect()
}

/// Everything fetched from the unit.
#[derive(Debug, Default)]
pub struct Stock {
    pub values: BTreeMap<String, String>,
    /// `[profile][weekday 0 = Monday]`, `None` when the file was not available.
    pub profiles: Vec<[Option<StockDay>; 7]>,
    /// Raw responses by file name, for `--save-raw`.
    pub raw: Vec<(String, String)>,
}

pub fn collect(src: &mut dyn Source) -> Result<Stock, String> {
    let mut stock = Stock::default();
    for page in PAGES {
        let file = page_file(page);
        if let Some(xml) = src.fetch(&file)? {
            if !xml.contains("<PARAMETER>") {
                return Err(format!("{file}: not a stock parameter document"));
            }
            stock.values.extend(parse_values(&xml));
            stock.raw.push((file, xml));
        }
    }
    for p in 0..PROFILES {
        let mut days: [Option<StockDay>; 7] = Default::default();
        for (d, day) in days.iter_mut().enumerate() {
            let file = profile_file(p, d + 1);
            if let Some(xml) = src.fetch(&file)? {
                *day = Some(parse_day(&xml));
                stock.raw.push((file, xml));
            }
        }
        stock.profiles.push(days);
    }
    if stock.values.is_empty() {
        return Err("no stock parameters found".into());
    }
    Ok(stock)
}

/// An assumption or a value that could not be taken over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note(pub String);

pub struct Import {
    pub config: Config,
    pub state: RuntimeState,
    pub notes: Vec<Note>,
}

struct Ctx<'a> {
    v: &'a BTreeMap<String, String>,
    notes: Vec<Note>,
}

impl Ctx<'_> {
    fn note(&mut self, msg: impl Into<String>) {
        self.notes.push(Note(msg.into()));
    }

    fn get(&self, id: &str) -> Option<&str> {
        self.v.get(id).map(String::as_str).filter(|s| !s.is_empty() && *s != "-")
    }

    fn num<T: std::str::FromStr>(&mut self, id: &str) -> Option<T> {
        let raw = self.get(id)?;
        match raw.replace(',', ".").parse() {
            Ok(v) => Some(v),
            Err(_) => {
                let msg = format!("{id} = {raw:?} is not a number; kept the default");
                self.note(msg);
                None
            }
        }
    }

    fn string<const N: usize>(&mut self, id: &str) -> Option<heapless::String<N>> {
        let raw = self.get(id)?;
        match heapless::String::try_from(raw) {
            Ok(s) => Some(s),
            Err(_) => {
                let msg = format!("{id} = {raw:?} is longer than {N} characters; kept the default");
                self.note(msg);
                None
            }
        }
    }
}

/// `13.07.2024` → date.
fn stock_date(s: &str) -> Option<Date> {
    let mut it = s.split('.');
    let (d, m, y) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    clock::Date::new(y, m, d).map(Date)
}

/// `HH:MM` → time of day (`24:00` is not a start time).
fn stock_time(s: &str) -> Option<TimeOfDay> {
    let (h, m) = s.split_once(':')?;
    TimeOfDay::new(h.parse().ok()?, m.parse().ok()?)
}

/// Stock country codes of the CET zone (the stock UI's countries are all CET/CEST).
const CET_COUNTRIES: [&str; 11] = ["DE", "AT", "CH", "FR", "IT", "PL", "SK", "SI", "HR", "HU", "NO"];

/// Maps stock parameters onto a default [`Config`]. `cwp` is the unit's `*.CWP` file for the
/// frost parameters (they are not exposed by the web UI).
pub fn import(stock: &Stock, cwp_file: Option<&[u8]>) -> Import {
    let mut c = Config::default();
    let mut state = RuntimeState::default();
    let mut x = Ctx { v: &stock.values, notes: Vec::new() };

    // Network (v01001–v01007).
    if let Some(dhcp) = x.num::<u8>("v01001") {
        c.network.dhcp = dhcp == 1;
    }
    if let Some(h) = x.string("v01007") {
        c.network.hostname = h;
    }
    c.network.address = x.string("v01002");
    c.network.netmask = x.string("v01003");
    c.network.gateway = x.string("v01004");
    c.network.dns = x.string("v01005");
    if x.get("v01006").is_some() {
        x.note("fallback DNS (v01006) is not supported; dropped");
    }

    // Time (v00051 GMT offset h, v00006 summer/winter, v02134 country).
    match x.get("v02134").map(str::to_owned) {
        Some(country) if CET_COUNTRIES.contains(&country.as_str()) => {
            c.time.utc_offset_min = 60;
            c.time.eu_dst = true;
            let stock_offset = x.get("v00051").unwrap_or("?").to_owned();
            x.note(format!(
                "country {country}: time zone set to CET with EU summer time (stock GMT offset {stock_offset} h ignored)"
            ));
        }
        _ => {
            if let Some(h) = x.num::<i16>("v00051") {
                c.time.utc_offset_min = h * 60;
            }
            c.time.eu_dst = x.num::<u8>("v00006") == Some(1);
            x.note("time zone taken from the stock GMT offset; check /config/time");
        }
    }

    // Fans (v00012/14/16/18 supply, v00013/15/17/19 extract).
    for level in 0..4 {
        let id = |base: usize| format!("v{:05}", base + 2 * level);
        if let Some(v) = x.num(&id(12)) {
            c.fans.supply_volts[level] = v;
        }
        if let Some(v) = x.num(&id(13)) {
            c.fans.extract_volts[level] = v;
        }
    }
    if c.fans.supply_volts != c.fans.extract_volts {
        x.note("supply/extract voltages differ; v000(12,14,16,18) were taken as supply (column order unconfirmed)");
    }
    x.note("air flow per level is not known from the stock unit; heat-recovery power stays unknown until set");

    // Programs (booster v00091/92, whisper v00096/97).
    let clamp_min = |m: u16| m.clamp(5, 180);
    if let Some(m) = x.num("v00091") {
        c.programs.party_min = clamp_min(m);
    }
    if let Some(l) = x.num::<u8>("v00092") {
        c.programs.party_level = l.min(4);
    }
    if let Some(m) = x.num("v00096") {
        c.programs.quiet_min = clamp_min(m);
    }
    if let Some(l) = x.num::<u8>("v00097") {
        c.programs.quiet_level = l.min(4);
    }

    // Bypass (v01035–37, passive cooling v02115: 0 n/a, 1 off, 2 on).
    if let Some(t) = x.num("v01035") {
        c.bypass.target_c = t;
    }
    if let Some(t) = x.num("v01036") {
        c.bypass.min_outdoor_c = t;
    }
    if let Some(t) = x.num("v01037") {
        c.bypass.supply_offset_k = t;
    }
    match x.num::<u8>("v02115") {
        Some(2) => c.bypass.enabled = true,
        Some(1) => c.bypass.enabled = false,
        _ => {}
    }
    let season = [x.get("v02120"), x.get("v02121"), x.get("v02128"), x.get("v02129")].map(|v| v.map(str::to_owned));
    if let [Some(d1), Some(m1), Some(d2), Some(m2)] = season {
        // The stock form validates days 1–31, but this unit stores day 0 (never edited). The
        // stock firmware evidently reads it as the start of the month: with the window
        // 0.3.–2.10. the bypass stayed closed on 16 October although it would have opened by
        // temperature. Day 0 is therefore imported as day 1.
        if d1 == "0" || d2 == "0" {
            x.note(format!("bypass season: stock day 0 read as day 1 (stored {d1}.{m1}. – {d2}.{m2}.)"));
        }
        let md = |d: &str, m: &str| clock::MonthDay::new(m.parse().ok()?, d.parse::<u8>().ok()?.max(1));
        match (md(&d1, &m1), md(&d2, &m2)) {
            (Some(s), Some(e)) => {
                c.bypass.season = Some(Season { start: openkwl_api::time::MonthDay(s), end: openkwl_api::time::MonthDay(e) })
            }
            _ => x.note(format!(
                "bypass season (day {d1}, month {m1} .. day {d2}, month {m2}) is not a valid date range; imported as all year"
            )),
        }
    }
    x.note("bypass actuator type is unknown; default 'motor' kept (check on the hardware)");

    // Frost parameters from the CWP file.
    match cwp_file.map(|b| cwp::frost_params(b, cwp::ParamSet::Base, false)) {
        Some(Ok(p)) => c.frost = p.into(),
        Some(Err(e)) => x.note(format!("CWP file not usable ({e:?}); stock frost defaults kept")),
        None => x.note("no CWP file given; stock frost defaults kept (identical on this unit)"),
    }

    // Holiday (v00601: 0 interval, 1 constant, 2 off).
    let start = x.get("v00603").and_then(stock_date);
    let end = x.get("v00604").and_then(stock_date);
    if let (Some(start), Some(end)) = (start, end) {
        c.holiday.start = start;
        c.holiday.end = end;
    }
    if let Some(l) = x.num::<u8>("v00602") {
        c.holiday.level = l.min(4);
    }
    match x.num::<u8>("v00601") {
        Some(0) => {
            c.holiday.enabled = true;
            if let Some(h) = x.num::<u8>("v00605") {
                c.holiday.interval_h = h.clamp(1, 24);
            }
            if let Some(m) = x.num::<u16>("v00606") {
                c.holiday.active_min = m.clamp(5, 300).min(u16::from(c.holiday.interval_h) * 60);
            }
        }
        Some(1) => {
            // Constant: the burst fills the whole interval.
            c.holiday.enabled = true;
            c.holiday.interval_h = 1;
            c.holiday.active_min = 60;
        }
        _ => c.holiday.enabled = false,
    }

    // Filter (v01032 months, v01033 remaining minutes).
    if let Some(m) = x.num::<u8>("v01032") {
        c.filter.interval_months = m;
    }
    if let Some(rem) = x.num::<u32>("v01033") {
        state.filter_elapsed_s = FilterCounter::elapsed_from_stock_remaining_min(c.filter.interval_months, rem);
        x.note(format!("filter: {rem} min remaining (≈ {} days)", rem / 1440));
    }

    // Counters (v01103/v01104 fan run time). Both this and v01033 count minutes: between two
    // captures 40 minutes apart (2026-10-16/17) each moved by exactly 40.
    let run_min = x.num::<u64>("v01103").into_iter().chain(x.num::<u64>("v01104")).max();
    if let Some(m) = run_min {
        state.operating_s = m * 60;
        x.note(format!("operating time: {m} fan minutes (≈ {} h)", m / 60));
    }

    // Mode (v00101: 0 automatic, 1 manual) and level.
    state.auto_mode = x.num::<u8>("v00101") != Some(1);
    if let Some(l) = x.num::<u8>("v00102") {
        state.manual_level = l.min(4);
    }

    // Schedule (v00901: profile 0–4, 5 = off; v02116 auto at midnight).
    match x.num::<u8>("v00901") {
        Some(p) if usize::from(p) < PROFILES => c.schedule.active_profile = p + 1,
        Some(_) => x.note("stock weekly program was 'Off'; profile 1 selected, mode left as imported"),
        None => {}
    }
    if let Some(v) = x.num::<u8>("v02116") {
        c.schedule.auto_at_midnight = v == 1;
    }
    for (p, days) in stock.profiles.iter().enumerate().take(PROFILES) {
        for (d, day) in days.iter().enumerate() {
            let Some(day) = day else { continue };
            let mut slots: heapless::Vec<Slot, { openkwl_core::schedule::MAX_SLOTS }> = heapless::Vec::new();
            for (from, _to, level) in day {
                match stock_time(from) {
                    Some(t) if slots.last().is_none_or(|s: &Slot| s.from < t) => {
                        let _ = slots.push(Slot { from: t, level: (*level).min(4) });
                    }
                    _ => x.note(format!("profile {} day {}: slot {from} skipped (unordered or invalid)", p + 1, d + 1)),
                }
            }
            c.profiles[p].days[d] = slots;
        }
    }

    // Device identity.
    let id = &mut c.device.identity;
    if let Some(v) = x.string("v00000") {
        id.model = v;
    }
    if let Some(v) = x.string("v00001") {
        id.article = v;
    }
    if let Some(v) = x.string("v00303") {
        id.serial = v;
    }
    if let Some(v) = x.string("v00304") {
        id.production_code = v;
    }
    if let Some(v) = x.string("v00343") {
        id.security_code = v;
    }
    match x.string::<17>("v00002") {
        Some(mac) => c.device.mac = mac,
        None => x.note("MAC address missing; the firmware will not start the network without one"),
    }

    // Anything the importer produced must load on the device.
    if let Err(e) = openkwl_config::ini::validate(&c) {
        x.note(format!("imported settings fail validation ({e:?}); fix CONFIG.INI before use"));
    }
    Import { config: c, state, notes: x.notes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stock_xml() {
        let xml = "<?xml version=\"1.0\"?>\n<PARAMETER>\n<LANG>de</LANG>\n<ID>v00002</ID>\n<VA>80:4B:20:12:34:56</VA>\n<ID>v01071</ID><VA>KWL%20FTF%20AD1</VA><ID>v02137</ID><VA>Int.%2BFF</VA><ID>v00108</ID><VA>-</VA></PARAMETER>";
        let v = parse_values(xml);
        assert_eq!(v[0], ("v00002".into(), "80:4B:20:12:34:56".into()));
        assert_eq!(v[1].1, "KWL FTF AD1");
        assert_eq!(v[2].1, "Int.+FF");
        assert_eq!(v[3].1, "-");
        assert_eq!(decode_value("100%"), "100%", "lone percent kept");
        assert_eq!(decode_value("a&amp;b"), "a&b");
    }

    #[test]
    fn stock_dates_and_times() {
        assert_eq!(stock_date("13.07.2024").unwrap().0, clock::Date::new(2024, 7, 13).unwrap());
        assert!(stock_date("31.02.2024").is_none());
        assert_eq!(stock_time("06:30"), TimeOfDay::new(6, 30));
        assert_eq!(stock_time("24:00"), None);
    }
}
