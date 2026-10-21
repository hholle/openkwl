//! The KWL-KNX module (`openkwl_bus::knx`): what the unit reports to it, and what it may
//! change. The platform moves the bus frames; see `firmware/src/bus.rs` and the simulator.

use openkwl_api::{Fault, Mode};
use openkwl_bus::knx::{DateTime, FanCommand, PROGRAMME_OFF, Push, Snapshot};
use openkwl_core::mode::Base;

use crate::App;

/// Stock error word: bit n−1 = "Fehler n".
fn error_bit(f: Fault) -> u32 {
    match f {
        // Fehler 01/02 (which fan is which is not confirmed).
        Fault::SupplyFanStall => 1 << 0,
        Fault::ExhaustFanStall => 1 << 1,
        // Fehler 04 SD card, Fehler 05 bus overcurrent.
        Fault::SdCard => 1 << 3,
        Fault::BusOvercurrent => 1 << 4,
        // Fehler 10–13 sensor missing, 14–17 short circuit (T1–T4).
        Fault::OutdoorSensorOpen => 1 << 9,
        Fault::SupplySensorOpen => 1 << 10,
        Fault::ExtractSensorOpen => 1 << 11,
        Fault::ExhaustSensorOpen => 1 << 12,
        Fault::OutdoorSensorShort => 1 << 13,
        Fault::SupplySensorShort => 1 << 14,
        Fault::ExtractSensorShort => 1 << 15,
        Fault::ExhaustSensorShort => 1 << 16,
        Fault::FilterChange | Fault::TimeNotSet | Fault::ConfigInvalid => 0,
    }
}

impl App {
    /// What the module gets in its blocks, in the stock layout.
    #[inline(never)]
    pub fn knx_snapshot(&self) -> Snapshot {
        let st = self.status();
        let c = &st.control;
        let mut status = 1; // running
        status |= match c.mode {
            Mode::Party => 1 << 4,
            Mode::Quiet => 1 << 5,
            _ => 0,
        };
        if st.holiday.is_some() {
            status |= 1 << 6;
        }
        if st.frost.active {
            status |= 1 << 25;
        }
        if st.filter.due {
            status |= 1 << 26;
        }
        let errors = st.faults.iter().fold(0, |e, &f| e | error_bit(f));
        let info = u32::from(st.filter.due) | u32::from(st.frost.active) << 1;
        let net = &self.connectivity.network;
        let network = if net.link_up { 0x10 } else { 0 }
            | match (net.dhcp, net.ip.is_some()) {
                (false, _) => 1,
                (true, false) => 2,
                (true, true) => 4,
            };
        let time = self.local_time().map(|t| DateTime {
            year: t.date.year,
            month: t.date.month,
            day: t.date.day,
            weekday: t.weekday() as u8 + 1,
            hour: (t.minute / 60) as u8,
            minute: (t.minute % 60) as u8,
            second: t.second,
        });
        let fan_minutes = (self.operating_s / 60).min(u64::from(u32::MAX)) as u32;
        let t = st.temperatures;
        Snapshot {
            level: c.level,
            auto: self.mode.base == Base::Auto,
            fan_pct: st.supply_fan.duty_pct,
            time,
            status,
            errors,
            warnings: 0,
            info,
            network,
            uptime_ms: (self.uptime_s * 1000) as u32,
            temperatures: [t.outdoor, t.supply, t.extract, t.exhaust],
            programme: self.config.schedule.active_profile.clamp(1, 5) - 1,
            filter_months: self.config.filter.interval_months,
            filter_remaining_min: self.filter.remaining_s() / 60,
            supply_minutes: fan_minutes,
            extract_minutes: fan_minutes,
        }
    }

    /// A level or Auto from KNX: like choosing it in the web UI (a level ends a running
    /// party/quiet program and counts as a manual change for the return to Auto; ignored while
    /// a holiday runs).
    pub fn knx_command(&mut self, cmd: FanCommand) {
        let r = match cmd {
            FanCommand::Auto => openkwl_api::ControlRequest { mode: Some(Mode::Auto), ..Default::default() },
            FanCommand::Manual(l) => openkwl_api::ControlRequest { mode: Some(Mode::Manual), level: Some(l), ..Default::default() },
        };
        self.log(format_args!("KNX: {}", if r.level.is_some() { "level" } else { "Auto" }));
        // Refused during a holiday, like the web UI and Home Assistant.
        let _ = self.apply_control(&r);
    }

    /// A block the module offered.
    pub fn knx_push(&mut self, push: Push) {
        match push {
            Push::Programme(p) if p < PROGRAMME_OFF => {
                self.config.schedule.active_profile = p + 1;
                self.config_changed();
                self.log(format_args!("KNX: weekly program {}", p + 1));
            }
            // "Off" has no counterpart: Auto always follows a weekly program here.
            Push::Programme(_) => self.log(format_args!("KNX: weekly program off ignored")),
        }
    }
}
