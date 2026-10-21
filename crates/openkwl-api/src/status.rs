//! `GET /status`: live readings, polled by the web UI and mirrored to MQTT.

use openkwl_core::frost;
use serde::{Deserialize, Serialize};

use crate::{
    control::Control,
    schedule::Slot,
    time::{Date, DateTime, TimeOfDay},
};

/// Most faults reported at once; further faults are dropped from the list.
pub const MAX_FAULTS: usize = 16;

/// Response of `GET /status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// Seconds since boot.
    pub uptime_s: u32,
    /// Local time, `null` while the clock is not set.
    pub time: Option<DateTime>,
    /// Active mode and level.
    pub control: Control,
    /// Today in the weekly program, `null` while the clock is not set.
    pub schedule: Option<ScheduleToday>,
    /// The holiday program, `null` unless today is between its dates.
    pub holiday: Option<HolidayStatus>,
    /// The four NTC sensors T1–T4.
    pub temperatures: Temperatures,
    /// Fan CH2 (PE11 / PB8). The CH2 = supply assignment is inferred.
    pub supply_fan: Fan,
    /// Fan CH1 (PE9 / PB9).
    pub exhaust_fan: Fan,
    /// Heat-recovery efficiency η, `null` until it can be computed.
    pub efficiency_pct: Option<f32>,
    /// Summer bypass.
    pub bypass: BypassStatus,
    /// Heat-exchanger frost protection.
    pub frost: FrostStatus,
    /// Recovered heat (estimate).
    pub heat_recovery: HeatRecovery,
    /// Filter-change reminder.
    pub filter: FilterStatus,
    /// Hours with at least one fan running.
    pub operating_hours: u32,
    /// Active faults and notices, empty when all is well.
    pub faults: heapless::Vec<Fault, MAX_FAULTS>,
}

/// Upper bound of the encoded [`Status`] (tested with worst-case values).
pub const STATUS_JSON_MAX: usize = 1792;

/// Most switching points in [`ScheduleToday::today`]: a day's slots plus the level carried
/// over from yesterday at 00:00.
pub const TODAY_SLOTS: usize = openkwl_core::schedule::MAX_SLOTS + 1;

/// Today in the active profile of the weekly program; it applies while the mode is auto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleToday {
    /// Active profile, 1–5.
    pub profile: u8,
    /// Today's switching points, sorted, starting at 00:00 when a level is carried over from
    /// yesterday (or an earlier day: days without slots continue the last level); empty when the profile has no slots at all.
    pub today: heapless::Vec<Slot, TODAY_SLOTS>,
    /// The next switching point with a different level, today or tomorrow; `null` if the level
    /// doesn't change before the end of tomorrow.
    pub next: Option<NextChange>,
}

/// The next level change of the weekly program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextChange {
    /// Time of the change, `"HH:MM"`.
    pub at: TimeOfDay,
    /// Level from then on, 0–4.
    pub level: u8,
    /// The change is tomorrow, not later today.
    pub tomorrow: bool,
}

/// A holiday program whose dates include today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayStatus {
    /// Last day (inclusive), `"YYYY-MM-DD"`.
    pub until: Date,
    /// A burst is running now (otherwise the fans rest until the next one).
    pub burst: bool,
}

/// Sensor readings in °C (stock sensor mapping); `null` for an open or shorted sensor (see `faults`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Temperatures {
    /// T1 Außenluft.
    pub outdoor: Option<f32>,
    /// T2 Zuluft.
    pub supply: Option<f32>,
    /// T3 Abluft.
    pub extract: Option<f32>,
    /// T4 Fortluft.
    pub exhaust: Option<f32>,
}

/// One EC fan: what is commanded and what the tacho measures.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fan {
    /// Commanded control voltage, 0–10 V (after frost boost / stop).
    pub volts: f32,
    /// PWM duty actually output, 0–100 % (0 below the 1.65 V cut-off).
    pub duty_pct: u8,
    /// Measured speed (8-sample average, 0 below 3 pulses/s).
    pub rpm: u32,
    /// Commanded but below the minimum speed for the stall time (stock Fehler 01/02).
    pub stalled: bool,
    /// Air flow in m³/h: the measured value per level from the settings, otherwise estimated
    /// from `rpm`; `null` when stopped or unknown.
    pub airflow_m3h: Option<f32>,
}

/// Summer bypass state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BypassStatus {
    /// Decision of the bypass controller.
    pub open: bool,
    /// The actuator is still running towards `open`.
    pub moving: bool,
}

/// Frost-protection outputs (as the stock frost protection behaves).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FrostStatus {
    /// State-machine state.
    pub state: FrostState,
    /// Protection engaged (stock status bit `0x1000000`).
    pub active: bool,
    /// The supply fan is forced off (defrost).
    pub supply_stopped: bool,
    /// Volts added to the exhaust fan setpoint.
    pub exhaust_boost_v: f32,
    /// "Info 2 – Frostschutz WT": a defrost cycle is running.
    pub defrost: bool,
}

impl From<frost::Outputs> for FrostStatus {
    fn from(o: frost::Outputs) -> Self {
        Self {
            state: o.state.into(),
            active: o.active,
            supply_stopped: o.stop_supply_fan,
            exhaust_boost_v: o.exhaust_boost_v,
            defrost: o.frost_info,
        }
    }
}

/// Heat-recovery estimate from configured air flow and T1/T2 ([`openkwl_core::heat`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeatRecovery {
    /// Current recovered power in W (negative = supply air cooled); `null` when air flow or
    /// temperatures are unknown, 0 while the bypass is open or the supply fan is off.
    pub power_w: Option<f32>,
    /// Heat recovered in total, kWh.
    pub heating_kwh: f32,
    /// Cooling recovered in total (summer), kWh.
    pub cooling_kwh: f32,
}

/// Filter-change reminder state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterStatus {
    /// Whole days of fan running time left, rounded up.
    pub remaining_days: u32,
    /// The interval has elapsed (stock "Info 1 – Luftfilter wechseln").
    pub due: bool,
}

/// Mirrors [`openkwl_core::frost::State`]; numbers are the stock state byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrostState {
    /// 0: not protecting.
    Idle,
    /// 1: icing suspected (η below entry threshold in the cold).
    Armed,
    /// 2: pre-heater path; never entered (no pre-heater fitted).
    PreheatWait,
    /// 3: exhaust boost applied.
    BoostStart,
    /// 4: checking whether η recovers.
    BoostCheck,
    /// 5: waiting out the boost delay with η still low.
    BoostHold,
    /// 6: deciding whether to defrost (outdoor < 3 °C).
    DefrostCheck,
    /// 7: supply fan off while the exchanger thaws.
    Defrost,
    /// 8: supply fan test run before re-measuring η.
    TestRun,
}

impl From<frost::State> for FrostState {
    fn from(s: frost::State) -> Self {
        match s {
            frost::State::Idle => FrostState::Idle,
            frost::State::Armed => FrostState::Armed,
            frost::State::PreheatWait => FrostState::PreheatWait,
            frost::State::BoostStart => FrostState::BoostStart,
            frost::State::BoostCheck => FrostState::BoostCheck,
            frost::State::BoostHold => FrostState::BoostHold,
            frost::State::DefrostCheck => FrostState::DefrostCheck,
            frost::State::Defrost => FrostState::Defrost,
            frost::State::TestRun => FrostState::TestRun,
        }
    }
}

/// Active faults. The stock error numbers ("Fehler 01/02", "Fehler 10–17") are noted where
/// the mapping is known; the per-sensor numbering of 10–17 is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// Supply fan stalled (Fehler 01/02 Drehzahlfehler; which number is which fan is unconfirmed).
    SupplyFanStall,
    /// Exhaust fan stalled (Fehler 01/02).
    ExhaustFanStall,
    /// T1 open (Fehler 10–13 "fehlt od. Kabelbruch").
    OutdoorSensorOpen,
    /// T2 open.
    SupplySensorOpen,
    /// T3 open.
    ExtractSensorOpen,
    /// T4 open.
    ExhaustSensorOpen,
    /// T1 shorted (Fehler 14–17 "Kurzschluss").
    OutdoorSensorShort,
    /// T2 shorted.
    SupplySensorShort,
    /// T3 shorted.
    ExtractSensorShort,
    /// T4 shorted.
    ExhaustSensorShort,
    /// Settings could not be read from or written to the SD card.
    SdCard,
    /// Notice: filter change due.
    FilterChange,
    /// Notice: clock not set (no SNTP yet); auto mode uses the fallback level and the bypass
    /// season keeps the bypass closed.
    TimeNotSet,
    /// `CONFIG.INI` failed to load; the last good copy is in use.
    ConfigInvalid,
    /// The module bus supply was switched off after repeated overcurrent (stock Fehler 05):
    /// extension modules such as the KWL-KNX have no power.
    BusOvercurrent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{BaseMode, Mode};
    use crate::test_util::round_trip;
    use openkwl_core::clock;

    pub(crate) const ALL_FAULTS: [Fault; 15] = [
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
    ];

    fn slot(from: u16, level: u8) -> Slot {
        Slot { from: TimeOfDay(from), level }
    }

    fn sample() -> Status {
        Status {
            uptime_s: 86_400,
            time: Some(DateTime::from(clock::TimeZone::CET.to_local(1_792_186_862))),
            control: Control {
                mode: Mode::Manual,
                level: 2,
                remaining_s: None,
                auto_return_s: Some(3600),
                base: BaseMode::Manual,
                manual_level: 2,
            },
            schedule: Some(ScheduleToday {
                profile: 1,
                today: heapless::Vec::from_slice(&[slot(0, 1), slot(390, 3), slot(480, 1)]).unwrap(),
                next: Some(NextChange { at: TimeOfDay(390), level: 3, tomorrow: true }),
            }),
            holiday: None,
            temperatures: Temperatures { outdoor: Some(-8.25), supply: Some(14.5), extract: Some(21.0), exhaust: None },
            supply_fan: Fan { volts: 4.5, duty_pct: 45, rpm: 1480, stalled: false, airflow_m3h: Some(150.0) },
            exhaust_fan: Fan { volts: 5.7, duty_pct: 57, rpm: 1720, stalled: false, airflow_m3h: Some(150.0) },
            efficiency_pct: Some(78.6),
            bypass: BypassStatus { open: false, moving: false },
            frost: FrostStatus {
                state: FrostState::BoostHold,
                active: true,
                supply_stopped: false,
                exhaust_boost_v: 1.2,
                defrost: false,
            },
            heat_recovery: HeatRecovery { power_w: Some(351.8), heating_kwh: 12_345.6, cooling_kwh: 0.0 },
            filter: FilterStatus { remaining_days: 50, due: false },
            operating_hours: 103_896,
            faults: heapless::Vec::from_slice(&[Fault::ExhaustSensorOpen]).unwrap(),
        }
    }

    #[test]
    fn status_round_trip() {
        let json = round_trip::<_, STATUS_JSON_MAX>(&sample());
        assert!(json.contains(r#""temperatures":{"outdoor":-8.25,"supply":14.5,"extract":21.0,"exhaust":null}"#), "{json}");
        assert!(json.contains(r#""state":"boost_hold""#));
        assert!(json.contains(r#""faults":["exhaust_sensor_open"]"#));
        assert!(json.contains(r#""time":"2026-10-16T23:41:02""#), "{json}");
        assert!(json.contains(r#""filter":{"remaining_days":50,"due":false}"#));
        assert!(json.contains(r#""next":{"at":"06:30","level":3,"tomorrow":true}"#), "{json}");
        assert!(json.contains(r#""holiday":null"#));
    }

    #[test]
    fn worst_case_fits_buffer() {
        let mut s = sample();
        s.uptime_s = u32::MAX;
        s.control.remaining_s = Some(u32::MAX);
        s.control.auto_return_s = Some(u32::MAX);
        s.control.manual_level = u8::MAX;
        s.schedule = Some(ScheduleToday {
            profile: u8::MAX,
            today: (0..TODAY_SLOTS as u16).map(|i| slot(1428 + i, u8::MAX)).collect(),
            next: Some(NextChange { at: TimeOfDay(1439), level: u8::MAX, tomorrow: false }),
        });
        s.holiday = Some(HolidayStatus { until: Date(clock::Date::new(2099, 12, 31).unwrap()), burst: false });
        s.temperatures = Temperatures {
            outdoor: Some(-123.456_79),
            supply: Some(-123.456_79),
            extract: Some(-123.456_79),
            exhaust: Some(-123.456_79),
        };
        let fan = Fan { volts: -1.234_567_9e-12, duty_pct: 100, rpm: u32::MAX, stalled: true, airflow_m3h: Some(-1.234_567_9e-12) };
        s.supply_fan = fan;
        s.exhaust_fan = fan;
        s.efficiency_pct = Some(-1.234_567_9e-12);
        s.frost.exhaust_boost_v = -1.234_567_9e-12;
        s.frost.state = FrostState::DefrostCheck;
        let x = -1.234_567_9e-12;
        s.heat_recovery = HeatRecovery { power_w: Some(x), heating_kwh: x, cooling_kwh: x };
        s.filter = FilterStatus { remaining_days: u32::MAX, due: true };
        s.operating_hours = u32::MAX;
        s.faults = heapless::Vec::from_slice(&ALL_FAULTS).unwrap();
        // Pad to capacity with the longest names.
        while s.faults.push(Fault::ExtractSensorShort).is_ok() {}
        round_trip::<_, STATUS_JSON_MAX>(&s);
    }

    #[test]
    fn frost_outputs_convert() {
        let mut c = frost::FrostController::new(frost::FrostParams::STOCK);
        let out = c.step(&frost::Inputs {
            t_outdoor: 5.0,
            t_supply: 15.0,
            t_extract: 21.0,
            supply_rpm: 1500,
            supply_min_rpm: 300,
            inhibit: false,
        });
        let fs = FrostStatus::from(out);
        assert_eq!(fs.state, FrostState::Idle);
        assert!(!fs.active && !fs.supply_stopped && !fs.defrost);
    }
}
