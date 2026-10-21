//! Imports the capture of the owner's unit (stock 2.27, 2026-10-16) from `testdata/`.

use openkwl_api::{Slot, time::TimeOfDay};
use openkwl_config::{Config, ini};
use openkwl_core::clock;
use openkwl_import::{DirSource, collect, import};

fn imported() -> openkwl_import::Import {
    let mut src = DirSource(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata").into());
    let stock = collect(&mut src).expect("fixtures present");
    import(&stock, None)
}

#[test]
fn takes_over_the_units_settings() {
    let r = imported();
    let c = &r.config;
    assert!(ini::validate(c).is_ok(), "{:?}", r.notes);

    assert!(c.network.dhcp);
    assert_eq!(c.network.hostname, "Helios");
    assert_eq!(c.network.address.as_deref(), Some("192.168.1.50"));
    assert_eq!((c.time.utc_offset_min, c.time.eu_dst), (60, true), "country DE");

    assert_eq!(c.fans.supply_volts, [5.0, 6.0, 7.0, 9.0]);
    assert_eq!(c.fans.extract_volts, [5.0, 6.0, 7.0, 9.0]);
    assert_eq!((c.programs.party_level, c.programs.party_min), (4, 150));
    assert_eq!((c.programs.quiet_level, c.programs.quiet_min), (0, 180));

    assert!(c.bypass.enabled, "passive cooling ON");
    assert_eq!((c.bypass.target_c, c.bypass.min_outdoor_c, c.bypass.supply_offset_k), (22.0, 7.0, 4.0));
    let season = c.bypass.season.expect("stock window 0.3.–2.10.");
    assert_eq!((season.start.0.month, season.start.0.day, season.end.0.month, season.end.0.day), (3, 1, 10, 2));

    let h = &c.holiday;
    assert!(h.enabled, "stock 'Constant'");
    assert_eq!((h.level, h.interval_h, h.active_min), (1, 1, 60));
    assert_eq!(h.start.0, clock::Date::new(2024, 7, 13).unwrap());

    assert_eq!(c.filter.interval_months, 3);
    assert_eq!((c.schedule.active_profile, c.schedule.auto_at_midnight), (2, true), "stock 'Standard 2'");

    assert_eq!(c.device.mac, "80:4B:20:12:34:56");
    assert_eq!((c.device.identity.serial.as_str(), c.device.identity.article.as_str()), ("1000", "04232001.hGP"));
}

#[test]
fn takes_over_the_weekly_program() {
    let c = imported().config;
    let s = |h, m, level| Slot { from: TimeOfDay::new(h, m).unwrap(), level };
    // Standard 2 (active), Monday.
    assert_eq!(
        c.profiles[1].days[0].as_slice(),
        [s(0, 0, 1), s(6, 30, 3), s(8, 0, 1), s(16, 0, 2), s(18, 0, 3), s(20, 30, 2), s(22, 30, 1)]
    );
    // Standard 1, Sunday.
    assert_eq!(c.profiles[0].days[6].as_slice(), [s(0, 0, 1), s(8, 30, 2), s(9, 30, 3), s(21, 0, 2), s(23, 0, 1)]);
    // User defined 2: level 1 all day.
    assert_eq!(c.profiles[4].days[3].as_slice(), [s(0, 0, 1)]);
}

#[test]
fn takes_over_counters_and_mode() {
    let st = imported().state;
    let days_left = (u32::from(3u8) * 30 * 86_400 - st.filter_elapsed_s) / 86_400;
    assert_eq!(days_left, 49, "71 867 min ≈ 49.9 days");
    assert_eq!(st.operating_s, 6_245_858 * 60, "larger of the two fan counters");
    assert!(st.auto_mode);
    assert_eq!(st.manual_level, 2);
}

#[test]
fn result_survives_the_ini_round_trip() {
    let c = imported().config;
    let mut text = String::new();
    ini::write_config(&c, &mut text).unwrap();
    let (back, report) = ini::read_config(&text, Config::default()).unwrap();
    assert_eq!(back, c);
    assert!(report.warnings.is_empty());
}

#[test]
fn notes_explain_assumptions() {
    let notes: Vec<String> = imported().notes.into_iter().map(|n| n.0).collect();
    let has = |s: &str| notes.iter().any(|n| n.contains(s));
    assert!(has("CET"), "{notes:?}");
    assert!(has("bypass season: stock day 0 read as day 1"));
    assert!(has("actuator"));
    assert!(has("fallback DNS"));
    assert!(!has("fail validation"), "{notes:?}");
}
