//! Heat-exchanger frost protection as the stock firmware behaves, variant without pre-heater.
//!
//! The stock controller watches the heat-recovery efficiency η. Icing on the exhaust side
//! makes η collapse, so a low η in cold weather is treated as icing:
//!
//! 1. Below `arm_below_c` outdoors with η < `entry_pct`: boost the exhaust fan.
//! 2. If η stays ≤ `hold_pct` − 0.1 for `boost_delay_s`: stop the supply fan.
//! 3. Below 3 °C outdoors: keep the supply fan off for `defrost_s` ("Info 2 – Frostschutz WT").
//! 4. Run the supply fan for `test_run_s`, stop it again and re-measure η; repeat from 3
//!    until η ≥ `exit_pct`.
//!
//! Observed quirk, kept on purpose: once armed, the controller only returns to [`State::Idle`]
//! through [`State::DefrostCheck`] (outdoor ≥ 3 °C) or a passed test run. While η stays above
//! `hold_pct` − 0.1 it cycles Armed → BoostStart → BoostCheck with the exhaust boost applied.
//! Confirm this on the stock unit before changing it.

use crate::efficiency::efficiency_pct;

/// Outdoor temperature below which a defrost cycle is allowed (fixed in the stock firmware).
pub const DEFROST_MAX_OUTDOOR_C: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrostParams {
    /// Outdoor temperature below which protection arms (CWP `0x1c`, −3.1 °C on this unit).
    pub arm_below_c: f32,
    /// Efficiency below which icing is assumed (CWP byte `A`, 58 %).
    pub entry_pct: f32,
    /// Efficiency that counts as "not recovering" (CWP byte `B`, used as B − 0.1 = 59.9 %).
    pub hold_pct: f32,
    /// Efficiency that ends protection after a test run (CWP byte `C`, 60 %).
    pub exit_pct: f32,
    /// Time below `hold_pct` before the supply fan stops (CWP `0x2a` minutes, 15 min).
    pub boost_delay_s: u32,
    /// Supply-fan off time per defrost (CWP `0x2c` minutes, 180 min).
    pub defrost_s: u32,
    /// Supply-fan test-run time (CWP `0x2e`, stored in seconds, 900 s).
    pub test_run_s: u32,
    /// Extra voltage on the exhaust fan while protecting (CWP `0x30`, 1.2 V).
    pub exhaust_boost_v: f32,
}

impl FrostParams {
    /// Values from the KWL EC 300 W R's parameter file `04232001.CWP` (all three parameter
    /// sets are identical there).
    pub const STOCK: Self = Self {
        arm_below_c: -3.1,
        entry_pct: 58.0,
        hold_pct: 60.0,
        exit_pct: 60.0,
        boost_delay_s: 15 * 60,
        defrost_s: 180 * 60,
        test_run_s: 900,
        exhaust_boost_v: 1.2,
    };

    fn hold_threshold(&self) -> f32 {
        // Computed in double precision, then narrowed, as the stock firmware does.
        (f64::from(self.hold_pct) - 0.1) as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    Idle = 0,
    Armed = 1,
    /// Pre-heater path; not supported (no pre-heater fitted), kept for numbering parity.
    PreheatWait = 2,
    BoostStart = 3,
    BoostCheck = 4,
    BoostHold = 5,
    DefrostCheck = 6,
    Defrost = 7,
    TestRun = 8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    pub t_outdoor: f32,
    pub t_supply: f32,
    pub t_extract: f32,
    pub supply_rpm: u32,
    /// Speed above which the supply fan counts as running.
    pub supply_min_rpm: u32,
    /// Skips the controller, like stock status bits `0x8800` (e.g. sensor faults).
    pub inhibit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outputs {
    pub state: State,
    /// Forces the supply fan to 0 V.
    pub stop_supply_fan: bool,
    /// Volts to add to the exhaust fan setpoint.
    pub exhaust_boost_v: f32,
    /// "Info 2 – Frostschutz WT".
    pub frost_info: bool,
    /// Stock status bit `0x1000000`: protection engaged.
    pub active: bool,
    pub efficiency_pct: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct FrostController {
    params: FrostParams,
    state: State,
    stop_supply: bool,
    boost_v: f32,
    info: bool,
    active: bool,
    eta: Option<f32>,
    boost_timer_s: u32,
    defrost_timer_s: u32,
    test_timer_s: u32,
}

impl FrostController {
    pub const fn new(params: FrostParams) -> Self {
        Self {
            params,
            state: State::Idle,
            stop_supply: false,
            boost_v: 0.0,
            info: false,
            active: false,
            eta: None,
            boost_timer_s: 0,
            defrost_timer_s: 0,
            test_timer_s: 0,
        }
    }

    pub fn params(&self) -> &FrostParams {
        &self.params
    }

    pub fn set_params(&mut self, params: FrostParams) {
        self.params = params;
    }

    /// Call once per second.
    pub fn tick_second(&mut self) {
        self.boost_timer_s = self.boost_timer_s.saturating_sub(1);
        self.defrost_timer_s = self.defrost_timer_s.saturating_sub(1);
        self.test_timer_s = self.test_timer_s.saturating_sub(1);
    }

    /// Runs one pass of the state machine (the stock firmware runs it continuously).
    pub fn step(&mut self, i: &Inputs) -> Outputs {
        if !i.inhibit {
            self.run(i);
        }
        self.outputs()
    }

    pub fn outputs(&self) -> Outputs {
        Outputs {
            state: self.state,
            stop_supply_fan: self.stop_supply,
            exhaust_boost_v: self.boost_v,
            frost_info: self.info,
            active: self.active,
            efficiency_pct: self.eta,
        }
    }

    fn run(&mut self, i: &Inputs) {
        let p = self.params;
        match self.state {
            State::Idle => {
                self.boost_v = 0.0;
                if i.t_outdoor < p.arm_below_c && i.supply_rpm > i.supply_min_rpm {
                    self.measure(i);
                    if self.eta.is_some_and(|eta| eta < p.entry_pct) {
                        self.state = State::Armed;
                        return;
                    }
                }
                self.active = false;
                self.boost_timer_s = 0;
            }
            State::Armed | State::PreheatWait => {
                self.active = true;
                self.state = State::BoostStart;
            }
            State::BoostStart => {
                self.boost_v = p.exhaust_boost_v;
                self.state = State::BoostCheck;
            }
            State::BoostCheck => {
                self.measure(i);
                if self.eta.is_some_and(|eta| eta > p.hold_threshold()) {
                    self.state = State::Armed;
                } else {
                    self.boost_timer_s = p.boost_delay_s;
                    self.state = State::BoostHold;
                }
            }
            State::BoostHold => {
                self.measure(i);
                if self.eta.is_some_and(|eta| eta <= p.hold_threshold()) {
                    if self.boost_timer_s == 0 {
                        self.stop_supply = true;
                        self.state = State::DefrostCheck;
                    }
                } else {
                    self.state = State::Armed;
                }
            }
            State::DefrostCheck => {
                if i.t_outdoor < DEFROST_MAX_OUTDOOR_C {
                    self.info = true;
                    self.defrost_timer_s = p.defrost_s;
                    self.state = State::Defrost;
                } else {
                    self.info = false;
                    self.stop_supply = false;
                    self.state = State::Idle;
                }
            }
            State::Defrost => {
                if self.defrost_timer_s == 0 {
                    self.test_timer_s = p.test_run_s;
                    self.stop_supply = false;
                    self.state = State::TestRun;
                }
            }
            State::TestRun => {
                if self.test_timer_s == 0 {
                    self.stop_supply = true;
                    self.measure(i);
                    if self.eta.is_some_and(|eta| eta < p.exit_pct) {
                        self.state = State::DefrostCheck;
                    } else {
                        self.state = State::Idle;
                        self.stop_supply = false;
                        self.boost_v = 0.0;
                        self.info = false;
                        self.active = false;
                    }
                }
            }
        }
    }

    fn measure(&mut self, i: &Inputs) {
        if let Some(eta) = efficiency_pct(i.t_outdoor, i.t_supply, i.t_extract) {
            self.eta = Some(eta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supply temperature that yields the requested efficiency with −8 °C out, 21 °C in.
    fn inputs(eta: f32, t_outdoor: f32) -> Inputs {
        let t_extract = 21.0;
        Inputs {
            t_outdoor,
            t_supply: t_outdoor + eta / 100.0 * (t_extract - t_outdoor),
            t_extract,
            supply_rpm: 1500,
            supply_min_rpm: 300,
            inhibit: false,
        }
    }

    fn run_for(c: &mut FrostController, i: &Inputs, seconds: u32) -> Outputs {
        let mut out = c.step(i);
        for _ in 0..seconds {
            c.tick_second();
            out = c.step(i);
        }
        out
    }

    #[test]
    fn stays_idle_when_mild() {
        let mut c = FrostController::new(FrostParams::STOCK);
        let out = run_for(&mut c, &inputs(40.0, 2.0), 60);
        assert_eq!(out.state, State::Idle);
        assert!(!out.stop_supply_fan && out.exhaust_boost_v == 0.0);
    }

    #[test]
    fn stays_idle_when_efficiency_is_fine() {
        let mut c = FrostController::new(FrostParams::STOCK);
        assert_eq!(run_for(&mut c, &inputs(80.0, -8.0), 60).state, State::Idle);
    }

    #[test]
    fn does_not_arm_with_supply_fan_stopped() {
        let mut c = FrostController::new(FrostParams::STOCK);
        let i = Inputs { supply_rpm: 0, ..inputs(40.0, -8.0) };
        assert_eq!(run_for(&mut c, &i, 60).state, State::Idle);
    }

    #[test]
    fn full_defrost_cycle() {
        let p = FrostParams::STOCK;
        let mut c = FrostController::new(p);
        let iced = inputs(40.0, -8.0);

        // Arms and boosts the exhaust fan immediately.
        let out = run_for(&mut c, &iced, 0);
        assert_eq!(out.state, State::Armed);
        let out = run_for(&mut c, &iced, 2);
        assert!(out.active);
        assert_eq!(out.exhaust_boost_v, 1.2);
        assert!(!out.stop_supply_fan);

        // After the boost delay the supply fan stops and a defrost starts.
        let out = run_for(&mut c, &iced, p.boost_delay_s + 1);
        assert_eq!(out.state, State::Defrost);
        assert!(out.stop_supply_fan && out.frost_info);

        // Supply stays off for the defrost time.
        let out = run_for(&mut c, &iced, p.defrost_s - 10);
        assert!(out.stop_supply_fan);
        let out = run_for(&mut c, &iced, 20);
        assert_eq!(out.state, State::TestRun);
        assert!(!out.stop_supply_fan, "test run restarts the supply fan");

        // Test run with recovered efficiency ends protection.
        let recovered = inputs(75.0, -8.0);
        let out = run_for(&mut c, &recovered, p.test_run_s + 1);
        assert_eq!(out.state, State::Idle);
        assert!(!out.stop_supply_fan && !out.frost_info && !out.active);
        assert_eq!(out.exhaust_boost_v, 0.0);
    }

    #[test]
    fn repeats_defrost_while_still_iced() {
        let p = FrostParams::STOCK;
        let mut c = FrostController::new(p);
        let iced = inputs(40.0, -8.0);
        run_for(&mut c, &iced, 3 + p.boost_delay_s + 1 + p.defrost_s + 1);
        let out = run_for(&mut c, &iced, p.test_run_s + 2);
        assert_eq!(out.state, State::Defrost, "η < 60 % after the test run");
        assert!(out.stop_supply_fan && out.frost_info);
    }

    #[test]
    fn no_defrost_above_three_degrees() {
        // Armed at 4 °C (threshold raised to 5 °C), but defrost needs < 3 °C: the controller
        // passes DefrostCheck back to Idle and re-arms, without ever starting a defrost.
        let p = FrostParams { arm_below_c: 5.0, ..FrostParams::STOCK };
        let mut c = FrostController::new(p);
        let i = inputs(40.0, 4.0);
        let mut saw_defrost_check = false;
        for _ in 0..(3 + p.boost_delay_s + 10) {
            let out = c.step(&i);
            saw_defrost_check |= out.state == State::DefrostCheck;
            assert_ne!(out.state, State::Defrost);
            assert!(!out.frost_info);
            c.tick_second();
        }
        assert!(saw_defrost_check);
    }

    #[test]
    fn documented_quirk_boost_persists_while_efficiency_recovers() {
        let mut c = FrostController::new(FrostParams::STOCK);
        run_for(&mut c, &inputs(40.0, -8.0), 2);
        // Efficiency recovers and it warms up, yet the stock logic keeps cycling with boost.
        let out = run_for(&mut c, &inputs(80.0, 0.0), 600);
        assert_ne!(out.state, State::Idle);
        assert_eq!(out.exhaust_boost_v, 1.2);
    }

    #[test]
    fn inhibit_freezes_the_state_machine() {
        let mut c = FrostController::new(FrostParams::STOCK);
        let i = Inputs { inhibit: true, ..inputs(40.0, -8.0) };
        assert_eq!(run_for(&mut c, &i, 60).state, State::Idle);
    }
}
