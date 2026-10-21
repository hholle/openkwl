//! The device application, shared by the firmware and the host simulator (`tools/openkwl-sim`).
//!
//! Everything between the hardware and the network lives here, so the simulator runs the
//! same code as the unit:
//!
//! - [`App::tick_second`]: the 1 Hz control loop. Sensor readings and fan speeds in, fan
//!   voltages and the bypass command out. It combines mode/schedule/holiday
//!   ([`openkwl_core::mode`]), frost protection, bypass, stall detection, counters.
//! - [`App::handle`] ([`http`]): the complete `/api/v1` REST API.
//! - [`App::take_writes`]: what to persist now, following the SD write budget; the platform
//!   writes the files with [`openkwl_config`].
//! - [`App::take_actions`]: requests for the platform (reboot, set the RTC).
//!
//! The platform supplies time (uptime and, if known, UTC), randomness for tokens/salts and
//! the I/O. `App` is `no_std` and allocation-free.

#![no_std]

pub mod buf;
pub mod http;
pub mod knx;
pub mod log;

use openkwl_api::{
    BypassStatus, Control, Fan, Fault, FilterStatus, FrostStatus, HeatRecovery, HolidayStatus, NextChange, ScheduleToday, Slot,
    Status, Temperatures,
    status::MAX_FAULTS,
    time::{Date, DateTime, TimeOfDay},
};
use openkwl_auth::{LoginThrottle, PasswordRecord, SessionStore};
use openkwl_config::{Config, Secrets, schedule::Debounce, schedule::Periodic, state::RuntimeState};
use openkwl_core::{
    airflow, bypass, clock,
    efficiency::efficiency_pct,
    fan,
    filter::FilterCounter,
    frost::{self, FrostController},
    heat::{EnergyCounter, recovered_power_w},
    mode::{Base, ModeState},
    ntc::Reading,
    schedule,
    tacho::StallDetector,
};

pub use http::{Hashed, Method, PasswordWork, Request, Response};

/// Concurrent sessions (browser tabs, Home Assistant is MQTT and needs none).
pub const SESSIONS: usize = 4;
pub const SESSION_IDLE_S: u64 = 30 * 60;
pub const SESSION_MAX_S: u64 = 12 * 3600;
/// "Stay logged in" sessions (separate slots, so they cannot push out normal ones).
pub const REMEMBERED_SESSIONS: usize = 4;
pub const REMEMBER_IDLE_S: u64 = 30 * 86_400;
pub const REMEMBER_MAX_S: u64 = 90 * 86_400;
/// First-time setup is accepted this long after power-on.
pub const SETUP_WINDOW_S: u64 = 30 * 60;
/// Supply fan speed above which frost protection treats it as running.
pub const SUPPLY_MIN_RPM: u32 = 300;
/// Stall detection: commanded on but below this speed for [`STALL_LIMIT_S`].
pub const STALL_MIN_RPM: u32 = 200;
pub const STALL_LIMIT_S: u32 = 60;

/// Measurements go to Home Assistant at most this often (switching values go at once). HA
/// records every state change and keeps 5-minute statistics, so faster brings nothing.
pub const HA_MEASUREMENT_MIN_S: u64 = 30;
/// Diagnostic values (fan speed, fan voltage) at most this often.
pub const HA_DIAGNOSTIC_MIN_S: u64 = 60;
/// Unchanged state is re-sent this often. Availability doesn't depend on it (retained state and
/// the last will cover that); it keeps the energy counters current within 5 minutes.
pub const HA_HEARTBEAT_S: u64 = 300;

/// Mode changes saved at once ([`Debounce::MODE`]) per day of uptime. Beyond it (a Home
/// Assistant automation switching levels all day) the mode waits for the 6-hourly save, a
/// restart or a power-fail save (write budget, `docs/PLAN.md` §3.1).
pub const MODE_SAVES_PER_DAY: u8 = 6;

/// What the platform measured this second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    /// T1 outdoor, T2 supply, T3 extract, T4 exhaust (already averaged and converted).
    pub outdoor: Reading,
    pub supply: Reading,
    pub extract: Reading,
    pub exhaust: Reading,
    pub supply_rpm: u32,
    pub exhaust_rpm: u32,
}

/// What the platform must drive.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Outputs {
    pub supply_volts: f32,
    pub exhaust_volts: f32,
    /// TIM1 duty 0–100 % (0 below the 1.65 V cut-off); enable the fan when > 0.
    pub supply_duty: u8,
    pub exhaust_duty: u8,
    /// Target bypass position; the actuator runs for `run_time_s` after a change.
    pub bypass_open: bool,
}

/// Static facts about the platform for `GET /system` and `GET /system/details`.
/// `firmware.uptime_s` and `memory.stack_peak_bytes` are filled in live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformInfo {
    pub firmware: openkwl_api::FirmwareDetails,
    pub chip: openkwl_api::ChipDetails,
    pub memory: openkwl_api::MemoryDetails,
    pub stock: openkwl_api::StockDetails,
    /// Clock source of the RTC: `LSE`, `LSI` or `host`.
    pub rtc_clock: heapless::String<8>,
}

/// Live platform state for `GET /system`, `GET /system/details` and faults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Connectivity {
    /// Link and IP state. `hostname` and `mac` are taken from the settings when empty.
    pub network: openkwl_api::NetworkDetails,
    pub sd_card: bool,
    pub sd_capacity_mb: Option<u32>,
    pub mqtt_connected: bool,
    pub mqtt_connects: u32,
    pub mqtt_published: u32,
    /// UTC of the last successful SNTP sync.
    pub last_time_sync_unix: Option<u64>,
    /// Server of the last SNTP attempt.
    pub time_server: heapless::String<64>,
    pub time_offset_ms: Option<i32>,
    pub time_syncs: u32,
    pub stack_peak_bytes: Option<u32>,
    pub board: openkwl_api::BoardDetails,
    /// Module bus supply and the KWL-KNX.
    pub modules: openkwl_api::ModuleDetails,
}

/// Counters for `GET /system/details` kept by the app itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    pub http_requests: u32,
    pub http_errors: u32,
    pub failed_logins: u32,
    pub mqtt_received: u32,
    pub writes: u32,
    pub last_state_save_unix: Option<u64>,
    pub time_set_manually: bool,
}

/// Everything loaded from the SD card at boot.
pub struct Boot {
    pub config: Config,
    pub secrets: Secrets,
    pub auth: Option<PasswordRecord>,
    pub state: RuntimeState,
    /// `CONFIG.INI` failed to load and defaults or `CONFIG.BAK` are in use.
    pub config_invalid: bool,
    pub platform: PlatformInfo,
}

/// Files the platform should write now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Writes {
    pub config: bool,
    pub secrets: bool,
    pub auth: bool,
    pub state: Option<RuntimeState>,
}

/// One-shot requests to the platform.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Actions {
    /// Restart after the current response has been sent (state is saved first).
    pub reboot: bool,
    /// Set the RTC to this UTC time.
    pub set_time_unix: Option<u64>,
    /// Reconnect MQTT with new settings.
    pub mqtt_changed: bool,
}

pub struct App {
    pub(crate) config: Config,
    pub(crate) secrets: Secrets,
    pub(crate) auth: Option<PasswordRecord>,
    pub(crate) sessions: SessionStore<SESSIONS>,
    pub(crate) remembered: SessionStore<REMEMBERED_SESSIONS>,
    pub(crate) throttle: LoginThrottle,
    pub(crate) mode: ModeState,
    frost: FrostController,
    supply_stall: StallDetector,
    exhaust_stall: StallDetector,
    energy: EnergyCounter,
    pub(crate) filter: FilterCounter,
    operating_s: u64,
    pub(crate) uptime_s: u64,
    pub(crate) unix_s: Option<u64>,
    /// Local day number and second of the day of the previous tick, to see midnight pass.
    last_local: Option<(u32, u32)>,
    scheduled: Option<u8>,
    holiday: Option<u8>,
    inputs: Option<Inputs>,
    outputs: Outputs,
    frost_out: Option<frost::Outputs>,
    bypass_moving_s: u32,
    heat_power_w: Option<f32>,
    /// Most recent non-zero level, for Home Assistant's "ON".
    last_level: u8,
    pub(crate) config_invalid: bool,
    pub(crate) platform: PlatformInfo,
    pub(crate) connectivity: Connectivity,
    pub(crate) stats: Stats,
    /// Last state sent to Home Assistant and when (uptime s).
    ha_last: Option<(openkwl_ha::State, u64)>,
    pub(crate) config_save: Debounce,
    /// Saves the state record soon after the mode or manual level changed, so a restart
    /// (watchdog, crash) comes back in the mode the user chose.
    mode_save: Debounce,
    saved_mode: (Base, u8),
    /// End (uptime s) of the current 24 h window and the mode saves made in it
    /// ([`MODE_SAVES_PER_DAY`]).
    mode_saves: (u64, u8),
    pub(crate) secrets_dirty: bool,
    pub(crate) auth_dirty: bool,
    state_save: Periodic,
    pub(crate) actions: Actions,
    pub(crate) log: log::Log,
}

impl App {
    pub fn new(boot: Boot) -> Self {
        let Boot { config, secrets, auth, state, config_invalid, platform } = boot;
        let mut mode = ModeState::new(if state.auto_mode { Base::Auto } else { Base::Manual }, state.manual_level);
        mode.set_auto_return(config.schedule.auto_return_s());
        let mut app = Self {
            frost: FrostController::new(config.frost.into()),
            filter: FilterCounter::new(config.filter.interval_months, state.filter_elapsed_s),
            config,
            secrets,
            auth,
            sessions: SessionStore::new(SESSION_IDLE_S, SESSION_MAX_S),
            remembered: SessionStore::new(REMEMBER_IDLE_S, REMEMBER_MAX_S),
            throttle: LoginThrottle::new(),
            mode,
            supply_stall: StallDetector::new(STALL_MIN_RPM, STALL_LIMIT_S),
            exhaust_stall: StallDetector::new(STALL_MIN_RPM, STALL_LIMIT_S),
            energy: EnergyCounter::from_raw(state.heating_j, state.cooling_j),
            operating_s: state.operating_s,
            uptime_s: 0,
            unix_s: None,
            last_local: None,
            scheduled: None,
            holiday: None,
            inputs: None,
            outputs: Outputs::default(),
            frost_out: None,
            bypass_moving_s: 0,
            heat_power_w: None,
            last_level: state.manual_level.max(1),
            config_invalid,
            platform,
            connectivity: Connectivity::default(),
            stats: Stats::default(),
            ha_last: None,
            config_save: Debounce::SETTINGS,
            mode_save: Debounce::MODE,
            saved_mode: (mode.base, mode.manual_level),
            mode_saves: (0, 0),
            secrets_dirty: false,
            auth_dirty: false,
            state_save: Periodic::STATE,
            actions: Actions::default(),
            log: log::Log::default(),
        };
        app.log(format_args!("started, {}", if app.auth.is_some() { "password set" } else { "setup mode" }));
        app
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    pub fn auth_record(&self) -> Option<&PasswordRecord> {
        self.auth.as_ref()
    }

    pub fn set_connectivity(&mut self, c: Connectivity) {
        self.connectivity = c;
    }

    pub(crate) fn log(&mut self, args: core::fmt::Arguments<'_>) {
        self.log.push(self.uptime_s, args);
    }

    pub fn local_time(&self) -> Option<clock::LocalDateTime> {
        self.unix_s.map(|t| self.config.time.zone().to_local(t))
    }

    /// Re-applies settings that live in controllers after a config change.
    pub(crate) fn config_changed(&mut self) {
        self.frost.set_params(self.config.frost.into());
        self.filter.set_interval_months(self.config.filter.interval_months);
        self.mode.set_auto_return(self.config.schedule.auto_return_s());
        self.recompute_mode_inputs();
        self.config_save.changed(self.uptime_s);
    }

    fn recompute_mode_inputs(&mut self) {
        let now = self.local_time();
        self.scheduled = now.and_then(|t| {
            let profile = self.config.profiles.get(usize::from(self.config.schedule.active_profile.max(1) - 1))?;
            schedule::level_at(|w| profile.core_day(w), t.weekday(), t.minute)
        });
        self.holiday = match (self.config.holiday.program(), now) {
            (Some(h), Some(t)) => h.level(&t),
            _ => None,
        };
    }

    pub fn control(&self) -> Control {
        Control::new(self.mode.resolve(self.scheduled, self.holiday, self.config.schedule.fallback_level), &self.mode)
    }

    /// Today in the active profile and its next level change, `None` while the clock is not set.
    fn schedule_today(&self) -> Option<ScheduleToday> {
        let t = self.local_time()?;
        let profile = self.config.schedule.active_profile.max(1);
        let days = self.config.profiles.get(usize::from(profile - 1))?;
        let week = |w: usize| days.core_day(w);
        let today = week(t.weekday());
        let slot = |s: &schedule::Slot| Slot { from: TimeOfDay(s.start_min), level: s.level };
        let mut slots = heapless::Vec::new();
        if today.first().is_none_or(|s| s.start_min > 0) {
            // The level carried into today: the last one of yesterday (or of an earlier day).
            let yesterday = (t.weekday() + 6) % 7;
            if let Some(level) = schedule::level_at(week, yesterday, clock::MINUTES_PER_DAY - 1) {
                let _ = slots.push(Slot { from: TimeOfDay(0), level });
            }
        }
        for s in &today {
            let _ = slots.push(slot(s));
        }
        // The API reports changes today or tomorrow; one further ahead is not "next" yet.
        let next = schedule::next_change(week, t.weekday(), t.minute)
            .filter(|&(_, days)| days <= 1)
            .map(|(s, days)| NextChange { at: TimeOfDay(s.start_min), level: s.level, tomorrow: days == 1 });
        Some(ScheduleToday { profile, today: slots, next })
    }

    /// The holiday program while today is between its dates.
    fn holiday_status(&self) -> Option<HolidayStatus> {
        let h = self.config.holiday.program()?;
        self.holiday.map(|level| HolidayStatus { until: Date(h.end), burst: level > 0 })
    }

    /// One control-loop pass. `unix_s` is the RTC (UTC) if it holds a valid time.
    pub fn tick_second(&mut self, inputs: &Inputs, unix_s: Option<u64>) -> Outputs {
        self.uptime_s += 1;
        self.unix_s = unix_s.filter(|&t| t >= clock::MIN_VALID_UNIX_S);
        self.inputs = Some(*inputs);

        // Midnight: back to auto if configured (stock v02116). Only when the clock runs into the
        // next day, a step of at most 2 minutes; a correction of the clock (SNTP, set by hand,
        // time zone) that lands on another day keeps the mode.
        if let Some(t) = self.local_time() {
            let now = (t.date.days(), u32::from(t.minute) * 60 + u32::from(t.second));
            let passed = self.last_local.is_some_and(|(day, second)| day + 1 == now.0 && now.1 + 86_400 - second <= 120);
            if passed && self.config.schedule.auto_at_midnight {
                self.mode.midnight_reset();
                self.log(format_args!("midnight: back to the weekly program"));
            }
            self.last_local = Some(now);
        }

        self.mode.tick_second();
        let m = (self.mode.base, self.mode.manual_level);
        if m != self.saved_mode {
            self.saved_mode = m;
            self.mode_save.changed(self.uptime_s);
        }
        self.recompute_mode_inputs();
        let level = self.control().level;
        if level > 0 {
            self.last_level = level;
        }
        let (mut supply_v, mut exhaust_v) = self.config.fans.volts(level);

        // Frost protection (as the stock firmware behaves): stop supply, boost exhaust.
        let (t_out, t_sup, t_ext) = (inputs.outdoor.celsius(), inputs.supply.celsius(), inputs.extract.celsius());
        self.frost.tick_second();
        let frost_out = self.frost.step(&frost::Inputs {
            t_outdoor: t_out.unwrap_or(0.0),
            t_supply: t_sup.unwrap_or(0.0),
            t_extract: t_ext.unwrap_or(0.0),
            supply_rpm: inputs.supply_rpm,
            supply_min_rpm: SUPPLY_MIN_RPM,
            inhibit: t_out.is_none() || t_sup.is_none() || t_ext.is_none(),
        });
        if level > 0 {
            if frost_out.stop_supply_fan {
                supply_v = 0.0;
            }
            exhaust_v = (exhaust_v + frost_out.exhaust_boost_v).min(10.0);
        }
        self.frost_out = Some(frost_out);

        // Bypass (as the stock firmware behaves), closed during frost protection or outside the season.
        let b = &self.config.bypass;
        let open = match (t_ext, t_out) {
            (Some(ext), Some(out)) if b.enabled && !frost_out.active && b.season_allows(self.local_time().map(|t| t.date)) => {
                bypass::decide(self.outputs.bypass_open, &b.params(), ext, out)
            }
            _ => false,
        };
        if open != self.outputs.bypass_open {
            self.bypass_moving_s = u32::from(b.run_time_s);
            self.log(format_args!("bypass {}", if open { "opening" } else { "closing" }));
        }
        self.bypass_moving_s = self.bypass_moving_s.saturating_sub(1);

        let supply_duty = fan::volts_to_duty(supply_v);
        let exhaust_duty = fan::volts_to_duty(exhaust_v);
        self.supply_stall.on_second(supply_duty, inputs.supply_rpm);
        self.exhaust_stall.on_second(exhaust_duty, inputs.exhaust_rpm);

        // Counters.
        let running = supply_duty > 0 || exhaust_duty > 0;
        self.filter.on_second(running);
        if running {
            self.operating_s += 1;
        }
        let airflow = self.supply_airflow_m3h(level, supply_duty, inputs.supply_rpm);
        self.heat_power_w = match (t_out, t_sup, airflow) {
            _ if supply_duty == 0 || open => Some(0.0),
            (Some(o), Some(s), Some(q)) => Some(recovered_power_w(q, o, s)),
            _ => None,
        };
        if let Some(p) = self.heat_power_w {
            self.energy.add_second(p);
        }

        self.outputs = Outputs {
            supply_volts: if supply_duty == 0 { 0.0 } else { supply_v },
            exhaust_volts: if exhaust_duty == 0 { 0.0 } else { exhaust_v },
            supply_duty,
            exhaust_duty,
            bypass_open: open,
        };
        self.outputs
    }

    fn runtime_state(&self) -> RuntimeState {
        let (heating_j, cooling_j) = self.energy.raw();
        RuntimeState {
            filter_elapsed_s: self.filter.elapsed_s(),
            heating_j,
            cooling_j,
            operating_s: self.operating_s,
            auto_mode: self.mode.base == Base::Auto,
            manual_level: self.mode.manual_level,
        }
    }

    /// Files to write now; call after every tick. Assumes the platform writes them.
    pub fn take_writes(&mut self) -> Writes {
        let now = self.uptime_s;
        let mut w = Writes::default();
        if self.config_save.due(now) {
            self.config_save.written();
            w.config = true;
        }
        w.secrets = core::mem::take(&mut self.secrets_dirty);
        w.auth = core::mem::take(&mut self.auth_dirty);
        if self.actions.reboot {
            self.state_save.force();
            w.config |= self.config_save.dirty();
            self.config_save.written();
        }
        if self.mode_save.due(now) {
            if now >= self.mode_saves.0 {
                self.mode_saves = (now + 86_400, 0);
            }
            // Over the daily cap the change stays pending for the next state save.
            if self.mode_saves.1 < MODE_SAVES_PER_DAY {
                self.mode_saves.1 += 1;
                self.state_save.force();
            }
        }
        if self.state_save.due(now) {
            self.state_save.written(now);
            // The record holds the current mode, so a pending mode save is done too.
            self.mode_save.written();
            w.state = Some(self.runtime_state());
            self.stats.last_state_save_unix = self.unix_s;
        }
        if w.config {
            // `CONFIG.INI` is rewritten from settings that are valid.
            self.config_invalid = false;
        }
        let files = [w.config, w.secrets, w.auth, w.state.is_some()];
        self.stats.writes = self.stats.writes.saturating_add(files.iter().filter(|&&f| f).count() as u32);
        w
    }

    /// Supply air flow: the measured value for the level from the settings, else estimated
    /// from the fan speed ([`airflow`]); `None` while the fan is off or nothing is known.
    fn supply_airflow_m3h(&self, level: u8, duty: u8, rpm: u32) -> Option<f32> {
        if duty == 0 {
            return None;
        }
        let configured = match level {
            1..=4 => self.config.fans.airflow_m3h[usize::from(level) - 1],
            _ => 0,
        };
        if configured > 0 {
            return Some(f32::from(configured));
        }
        airflow::estimate_m3h(rpm, self.config.fans.m3h_per_krpm)
    }

    /// Restarts the filter interval and saves it with the next [`App::take_writes`]: the
    /// 6-hourly save would bring the old count back after a restart in between. A rare action,
    /// one 64-byte state write each.
    pub(crate) fn filter_reset(&mut self) {
        self.filter.reset();
        self.state_save.force();
    }

    /// Power-fail warning: save counters with the next [`App::take_writes`].
    pub fn power_failing(&mut self) {
        self.state_save.force();
    }

    pub fn take_actions(&mut self) -> Actions {
        core::mem::take(&mut self.actions)
    }

    /// State document for Home Assistant (`<base>/state`).
    pub fn ha_state(&self) -> openkwl_ha::State {
        openkwl_ha::State::new(&self.status(), &self.config.bypass)
    }

    /// The state to publish to Home Assistant now, or `None`. Call about once a second:
    /// - switching values (mode, level, bypass, frost, faults…) go out at once;
    /// - measurements that moved beyond their dead band at most every [`HA_MEASUREMENT_MIN_S`],
    ///   diagnostics (fan speed, voltage) at most every [`HA_DIAGNOSTIC_MIN_S`];
    /// - otherwise a heartbeat every [`HA_HEARTBEAT_S`].
    ///
    /// `force` publishes regardless (after a command from Home Assistant or a reconnect).
    pub fn ha_state_due(&mut self, force: bool) -> Option<openkwl_ha::State> {
        use openkwl_ha::Change;
        let now = self.uptime_s;
        let state = self.ha_state();
        let due = force
            || match &self.ha_last {
                None => true,
                Some((prev, at)) => match state.change_from(prev) {
                    Change::State => true,
                    Change::Measurement => now >= at + HA_MEASUREMENT_MIN_S,
                    Change::Diagnostic => now >= at + HA_DIAGNOSTIC_MIN_S,
                    Change::None => now >= at + HA_HEARTBEAT_S,
                },
            };
        if !due {
            return None;
        }
        self.ha_last = Some((state.clone(), now));
        Some(state)
    }

    /// Applies a message from one of the MQTT command topics. Returns `false` for unknown
    /// topics, malformed payloads and values the REST API would reject. A level or preset
    /// during a holiday is refused like in the REST API but returns `true`: the platform then
    /// republishes the state at once, so Home Assistant shows the holiday again.
    pub fn ha_command(&mut self, topic: &str, payload: &[u8]) -> bool {
        use openkwl_ha::Command;
        let Ok(cmd) = Command::parse(&self.config.mqtt.base_topic, topic, payload) else {
            return false;
        };
        self.stats.mqtt_received = self.stats.mqtt_received.saturating_add(1);
        if let Some(req) = cmd.to_request(self.last_level) {
            use openkwl_api::Validate as _;
            if req.validate().is_err() {
                return false;
            }
            if self.apply_control(&req).is_err() {
                self.log(format_args!("MQTT command ignored: holiday running"));
            }
            return true;
        }
        match cmd {
            Command::BypassEnabled(on) => self.config.bypass.enabled = on,
            Command::BypassTarget(t) => {
                use openkwl_api::Validate as _;
                let next = openkwl_api::BypassConfig { target_c: t, ..self.config.bypass };
                if next.validate().is_err() {
                    return false;
                }
                self.config.bypass = next;
            }
            Command::FilterReset => {
                self.filter_reset();
                self.log(format_args!("filter interval restarted (MQTT)"));
                return true;
            }
            Command::Reboot => {
                self.actions.reboot = true;
                self.log(format_args!("restart requested (MQTT)"));
                return true;
            }
            _ => return false,
        }
        self.config_changed();
        self.log(format_args!("bypass settings changed (MQTT)"));
        true
    }

    // Called from the API, Home Assistant and the KNX module: one copy, not one per caller.
    #[inline(never)]
    pub fn status(&self) -> Status {
        let i = self.inputs;
        let reading = |f: fn(&Inputs) -> Reading| i.as_ref().map_or(Reading::Open, f);
        let temps = [reading(|i| i.outdoor), reading(|i| i.supply), reading(|i| i.extract), reading(|i| i.exhaust)];
        let mut faults: heapless::Vec<Fault, MAX_FAULTS> = heapless::Vec::new();
        let mut fault = |f| {
            let _ = faults.push(f);
        };
        if self.supply_stall.stalled() {
            fault(Fault::SupplyFanStall);
        }
        if self.exhaust_stall.stalled() {
            fault(Fault::ExhaustFanStall);
        }
        const OPEN: [Fault; 4] =
            [Fault::OutdoorSensorOpen, Fault::SupplySensorOpen, Fault::ExtractSensorOpen, Fault::ExhaustSensorOpen];
        const SHORT: [Fault; 4] =
            [Fault::OutdoorSensorShort, Fault::SupplySensorShort, Fault::ExtractSensorShort, Fault::ExhaustSensorShort];
        if i.is_some() {
            for (k, r) in temps.iter().enumerate() {
                match r {
                    Reading::Open => fault(OPEN[k]),
                    Reading::Short => fault(SHORT[k]),
                    Reading::Celsius(_) => {}
                }
            }
        }
        if !self.connectivity.sd_card {
            fault(Fault::SdCard);
        }
        if self.filter.due() {
            fault(Fault::FilterChange);
        }
        if self.unix_s.is_none() {
            fault(Fault::TimeNotSet);
        }
        if self.config_invalid {
            fault(Fault::ConfigInvalid);
        }
        if self.connectivity.modules.bus_power == openkwl_api::BusPower::Overcurrent {
            fault(Fault::BusOvercurrent);
        }
        let [o, s, e, x] = temps.map(Reading::celsius);
        let level = self.control().level;
        let fan = |volts: f32, duty: u8, rpm: u32, stalled: bool, airflow_m3h: Option<f32>| Fan {
            volts,
            duty_pct: duty,
            rpm,
            stalled,
            airflow_m3h,
        };
        let rpm = |f: fn(&Inputs) -> u32| i.as_ref().map_or(0, f);
        let fo = self.frost_out.unwrap_or_else(|| self.frost.outputs());
        Status {
            uptime_s: self.uptime_s.min(u64::from(u32::MAX)) as u32,
            time: self.local_time().map(DateTime::from),
            control: self.control(),
            schedule: self.schedule_today(),
            holiday: self.holiday_status(),
            temperatures: Temperatures { outdoor: o, supply: s, extract: e, exhaust: x },
            supply_fan: fan(
                self.outputs.supply_volts,
                self.outputs.supply_duty,
                rpm(|i| i.supply_rpm),
                self.supply_stall.stalled(),
                self.supply_airflow_m3h(level, self.outputs.supply_duty, rpm(|i| i.supply_rpm)),
            ),
            exhaust_fan: fan(
                self.outputs.exhaust_volts,
                self.outputs.exhaust_duty,
                rpm(|i| i.exhaust_rpm),
                self.exhaust_stall.stalled(),
                (self.outputs.exhaust_duty > 0)
                    .then(|| airflow::estimate_m3h(rpm(|i| i.exhaust_rpm), self.config.fans.m3h_per_krpm))
                    .flatten(),
            ),
            efficiency_pct: match (o, s, e) {
                (Some(o), Some(s), Some(e)) if self.outputs.supply_duty > 0 => efficiency_pct(o, s, e),
                _ => None,
            },
            bypass: BypassStatus { open: self.outputs.bypass_open, moving: self.bypass_moving_s > 0 },
            frost: FrostStatus::from(fo),
            heat_recovery: HeatRecovery {
                power_w: self.heat_power_w,
                heating_kwh: self.energy.heating_kwh(),
                cooling_kwh: self.energy.cooling_kwh(),
            },
            filter: FilterStatus { remaining_days: self.filter.remaining_days(), due: self.filter.due() },
            operating_hours: (self.operating_s / 3600).min(u64::from(u32::MAX)) as u32,
            faults,
        }
    }
}

#[cfg(test)]
mod tests;
