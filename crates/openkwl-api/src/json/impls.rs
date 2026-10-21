//! [`FromJson`] for every request body: keys to fields, in the order of the struct (the bit in
//! [`Seen`]). Optional fields may be missing or `null`; all others are required.

use super::{Error, FromJson, Reader, Result, Seen};
use crate::{
    config::{
        ActuatorType, BypassConfig, FanConfig, FilterConfig, FrostConfig, HolidayConfig, MqttConfigUpdate, NetworkConfig,
        ProgramConfig, Season, TimeConfig,
    },
    control::{ControlRequest, Mode},
    schedule::{Day, ScheduleConfig, ScheduleProfile, Slot},
    system::SetTimeRequest,
    time::{Date, MonthDay, TimeOfDay},
};

/// A string parsed by the type's own `TryFrom` (dates, times).
fn parsed<T: TryFrom<heapless::String<N>>, const N: usize>(r: &mut Reader<'_>) -> Result<T> {
    T::try_from(r.string::<N>()?).map_err(|_| Error)
}

/// `null` (or a missing key) is `None`.
fn optional<'a, T>(r: &mut Reader<'a>, read: impl FnOnce(&mut Reader<'a>) -> Result<T>) -> Result<Option<T>> {
    if r.null() { Ok(None) } else { read(r).map(Some) }
}

/// Exactly `N` numbers.
fn f32s<const N: usize>(r: &mut Reader<'_>) -> Result<[f32; N]> {
    let mut v = [0.0; N];
    let mut n = 0;
    r.array(&mut |r, i| {
        *v.get_mut(i).ok_or(Error)? = r.f32()?;
        n = i + 1;
        Ok(())
    })?;
    if n == N { Ok(v) } else { Err(Error) }
}

fn u16s<const N: usize>(r: &mut Reader<'_>) -> Result<[u16; N]> {
    let mut v = [0; N];
    let mut n = 0;
    r.array(&mut |r, i| {
        *v.get_mut(i).ok_or(Error)? = r.u16()?;
        n = i + 1;
        Ok(())
    })?;
    if n == N { Ok(v) } else { Err(Error) }
}

/// One of the names, as its index.
fn name(r: &mut Reader<'_>, names: &[&str]) -> Result<usize> {
    let mut buf = [0u8; 16];
    let s = r.str(&mut buf)?;
    names.iter().position(|n| *n == s).ok_or(Error)
}

impl FromJson for Season {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let (mut start, mut end) = (None, None);
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"start" => (seen.mark(0)?, start = Some(parsed::<MonthDay, 5>(r)?)),
                b"end" => (seen.mark(1)?, end = Some(parsed::<MonthDay, 5>(r)?)),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(2)?;
        Ok(Season { start: start.ok_or(Error)?, end: end.ok_or(Error)? })
    }
}

impl FromJson for BypassConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = BypassConfig {
            enabled: false,
            target_c: 0.0,
            min_outdoor_c: 0.0,
            close_if_outdoor_warmer: false,
            actuator: ActuatorType::Motor,
            run_time_s: 0,
            season: None,
            supply_offset_k: 0.0,
        };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"enabled" => (seen.mark(0)?, v.enabled = r.bool()?),
                b"target_c" => (seen.mark(1)?, v.target_c = r.f32()?),
                b"min_outdoor_c" => (seen.mark(2)?, v.min_outdoor_c = r.f32()?),
                b"close_if_outdoor_warmer" => (seen.mark(3)?, v.close_if_outdoor_warmer = r.bool()?),
                b"actuator" => {
                    const NAMES: [&str; 3] = ["analog", "motor", "motor_reversed"];
                    const TYPES: [ActuatorType; 3] = [ActuatorType::Analog, ActuatorType::Motor, ActuatorType::MotorReversed];
                    (seen.mark(4)?, v.actuator = TYPES[name(r, &NAMES)?])
                }
                b"run_time_s" => (seen.mark(5)?, v.run_time_s = r.u16()?),
                b"season" => (seen.mark(7)?, v.season = optional(r, Season::from_json)?),
                b"supply_offset_k" => (seen.mark(6)?, v.supply_offset_k = r.f32()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(7)?; // `season` (bit 7) is optional
        Ok(v)
    }
}

impl FromJson for FrostConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = FrostConfig {
            arm_below_c: 0.0,
            entry_pct: 0.0,
            hold_pct: 0.0,
            exit_pct: 0.0,
            boost_delay_s: 0,
            defrost_s: 0,
            test_run_s: 0,
            exhaust_boost_v: 0.0,
        };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"arm_below_c" => (seen.mark(0)?, v.arm_below_c = r.f32()?),
                b"entry_pct" => (seen.mark(1)?, v.entry_pct = r.f32()?),
                b"hold_pct" => (seen.mark(2)?, v.hold_pct = r.f32()?),
                b"exit_pct" => (seen.mark(3)?, v.exit_pct = r.f32()?),
                b"boost_delay_s" => (seen.mark(4)?, v.boost_delay_s = r.u32()?),
                b"defrost_s" => (seen.mark(5)?, v.defrost_s = r.u32()?),
                b"test_run_s" => (seen.mark(6)?, v.test_run_s = r.u32()?),
                b"exhaust_boost_v" => (seen.mark(7)?, v.exhaust_boost_v = r.f32()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(8)?;
        Ok(v)
    }
}

impl FromJson for FanConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = FanConfig { supply_volts: [0.0; 4], extract_volts: [0.0; 4], airflow_m3h: [0; 4], m3h_per_krpm: 0 };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"supply_volts" => (seen.mark(0)?, v.supply_volts = f32s(r)?),
                b"extract_volts" => (seen.mark(1)?, v.extract_volts = f32s(r)?),
                b"airflow_m3h" => (seen.mark(2)?, v.airflow_m3h = u16s(r)?),
                b"m3h_per_krpm" => (seen.mark(3)?, v.m3h_per_krpm = r.u16()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(4)?;
        Ok(v)
    }
}

impl FromJson for ProgramConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = ProgramConfig { party_level: 0, party_min: 0, quiet_level: 0, quiet_min: 0 };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"party_level" => (seen.mark(0)?, v.party_level = r.u8()?),
                b"party_min" => (seen.mark(1)?, v.party_min = r.u16()?),
                b"quiet_level" => (seen.mark(2)?, v.quiet_level = r.u8()?),
                b"quiet_min" => (seen.mark(3)?, v.quiet_min = r.u16()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(4)?;
        Ok(v)
    }
}

impl FromJson for HolidayConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let (mut enabled, mut start, mut end, mut level, mut interval_h, mut active_min) = (false, None, None, 0, 0, 0);
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"enabled" => (seen.mark(0)?, enabled = r.bool()?),
                b"start" => (seen.mark(1)?, start = Some(parsed::<Date, 10>(r)?)),
                b"end" => (seen.mark(2)?, end = Some(parsed::<Date, 10>(r)?)),
                b"level" => (seen.mark(3)?, level = r.u8()?),
                b"interval_h" => (seen.mark(4)?, interval_h = r.u8()?),
                b"active_min" => (seen.mark(5)?, active_min = r.u16()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(6)?;
        Ok(HolidayConfig { enabled, start: start.ok_or(Error)?, end: end.ok_or(Error)?, level, interval_h, active_min })
    }
}

impl FromJson for TimeConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = TimeConfig { ntp_enabled: false, ntp_server: heapless::String::new(), utc_offset_min: 0, eu_dst: false };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"ntp_enabled" => (seen.mark(0)?, v.ntp_enabled = r.bool()?),
                b"ntp_server" => (seen.mark(1)?, v.ntp_server = r.string()?),
                b"utc_offset_min" => (seen.mark(2)?, v.utc_offset_min = r.i16()?),
                b"eu_dst" => (seen.mark(3)?, v.eu_dst = r.bool()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(4)?;
        Ok(v)
    }
}

impl FromJson for FilterConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = FilterConfig { interval_months: 0 };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"interval_months" => (seen.mark(0)?, v.interval_months = r.u8()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(1)?;
        Ok(v)
    }
}

impl FromJson for NetworkConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v =
            NetworkConfig { dhcp: false, hostname: heapless::String::new(), address: None, netmask: None, gateway: None, dns: None };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"dhcp" => (seen.mark(0)?, v.dhcp = r.bool()?),
                b"hostname" => (seen.mark(1)?, v.hostname = r.string()?),
                b"address" => (seen.mark(2)?, v.address = optional(r, Reader::string)?),
                b"netmask" => (seen.mark(3)?, v.netmask = optional(r, Reader::string)?),
                b"gateway" => (seen.mark(4)?, v.gateway = optional(r, Reader::string)?),
                b"dns" => (seen.mark(5)?, v.dns = optional(r, Reader::string)?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(2)?; // the addresses are optional
        Ok(v)
    }
}

impl FromJson for MqttConfigUpdate {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = MqttConfigUpdate {
            enabled: false,
            host: heapless::String::new(),
            port: 0,
            username: heapless::String::new(),
            password: None,
            client_id: heapless::String::new(),
            base_topic: heapless::String::new(),
            discovery: false,
            discovery_prefix: heapless::String::new(),
        };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"enabled" => (seen.mark(0)?, v.enabled = r.bool()?),
                b"host" => (seen.mark(1)?, v.host = r.string()?),
                b"port" => (seen.mark(2)?, v.port = r.u16()?),
                b"username" => (seen.mark(3)?, v.username = r.string()?),
                b"client_id" => (seen.mark(4)?, v.client_id = r.string()?),
                b"base_topic" => (seen.mark(5)?, v.base_topic = r.string()?),
                b"discovery" => (seen.mark(6)?, v.discovery = r.bool()?),
                b"discovery_prefix" => (seen.mark(7)?, v.discovery_prefix = r.string()?),
                b"password" => (seen.mark(8)?, v.password = optional(r, Reader::string)?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(8)?; // `password` (bit 8) is optional
        Ok(v)
    }
}

impl FromJson for ScheduleConfig {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = ScheduleConfig { active_profile: 0, auto_at_midnight: false, auto_return_min: None, fallback_level: 0 };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"active_profile" => (seen.mark(0)?, v.active_profile = r.u8()?),
                b"auto_at_midnight" => (seen.mark(1)?, v.auto_at_midnight = r.bool()?),
                b"fallback_level" => (seen.mark(2)?, v.fallback_level = r.u8()?),
                b"auto_return_min" => (seen.mark(3)?, v.auto_return_min = optional(r, Reader::u16)?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(3)?; // `auto_return_min` (bit 3) is optional
        Ok(v)
    }
}

impl FromJson for Slot {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let (mut from, mut level) = (None, 0);
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"from" => (seen.mark(0)?, from = Some(parsed::<TimeOfDay, 5>(r)?)),
                b"level" => (seen.mark(1)?, level = r.u8()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(2)?;
        Ok(Slot { from: from.ok_or(Error)?, level })
    }
}

impl FromJson for ScheduleProfile {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut days: [Day; 7] = Default::default();
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            if k != b"days" {
                return r.skip();
            }
            seen.mark(0)?;
            let mut n = 0;
            r.array(&mut |r, d| {
                let day = days.get_mut(d).ok_or(Error)?;
                n = d + 1;
                r.array(&mut |r, _| day.push(Slot::from_json(r)?).map_err(|_| Error))
            })?;
            if n == 7 { Ok(()) } else { Err(Error) }
        })?;
        seen.all(1)?;
        Ok(ScheduleProfile { days })
    }
}

impl FromJson for ControlRequest {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = ControlRequest { mode: None, level: None, duration_min: None, end_program: false };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"mode" => {
                    const NAMES: [&str; 5] = ["manual", "auto", "party", "quiet", "holiday"];
                    const MODES: [Mode; 5] = [Mode::Manual, Mode::Auto, Mode::Party, Mode::Quiet, Mode::Holiday];
                    (seen.mark(0)?, v.mode = optional(r, |r| Ok(MODES[name(r, &NAMES)?]))?)
                }
                b"level" => (seen.mark(1)?, v.level = optional(r, Reader::u8)?),
                b"duration_min" => (seen.mark(2)?, v.duration_min = optional(r, Reader::u16)?),
                b"end_program" => (seen.mark(3)?, v.end_program = r.bool()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        Ok(v)
    }
}

impl FromJson for SetTimeRequest {
    fn from_json(r: &mut Reader<'_>) -> Result<Self> {
        let mut v = SetTimeRequest { unix_s: 0 };
        let mut seen = Seen::default();
        r.object(&mut |r, k| {
            match k {
                b"unix_s" => (seen.mark(0)?, v.unix_s = r.u64()?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        seen.all(1)?;
        Ok(v)
    }
}
