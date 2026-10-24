//! Sensors (ADC + NTC), tachometers (EXTI) and outputs (TIM1 PWM, enables, bypass drive).

use core::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use embassy_stm32::{
    adc::{Adc, AnyAdcChannel, SampleTime},
    exti::ExtiInput,
    gpio::{Level, Output},
    mode::Async,
    peripherals::{ADC1, TIM1},
    timer::{Channel, simple_pwm::SimplePwm},
};
use embassy_sync::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    signal::Signal,
};
use embassy_time::{Duration, Instant, Ticker, Timer, with_timeout};
use openkwl_api::ActuatorType;
use openkwl_app::{Inputs, Outputs};
use openkwl_core::{average::MovingAverage, ntc, tacho};

use crate::board;

/// Latest sensor readings, written by [`sensor_task`], read by the control loop.
pub static INPUTS: Mutex<CriticalSectionRawMutex, Cell<Inputs>> = Mutex::new(Cell::new(Inputs {
    outdoor: ntc::Reading::Open,
    supply: ntc::Reading::Open,
    extract: ntc::Reading::Open,
    exhaust: ntc::Reading::Open,
    supply_rpm: 0,
    exhaust_rpm: 0,
}));

pub static SUPPLY_RPM: AtomicU32 = AtomicU32::new(0);
pub static EXHAUST_RPM: AtomicU32 = AtomicU32::new(0);

pub fn inputs() -> Inputs {
    let mut i = INPUTS.lock(Cell::get);
    i.supply_rpm = SUPPLY_RPM.load(Ordering::Relaxed);
    i.exhaust_rpm = EXHAUST_RPM.load(Ordering::Relaxed);
    i
}

/// Round-robin over the four NTCs every 250 ms with an 8-sample moving average, like the
/// stock firmware. The NTC pull-up supply is measured on PC3 each round
/// and used for the conversion, as stock does, once its average is filled.
#[embassy_executor::task]
pub async fn sensor_task(
    mut adc: Adc<'static, ADC1>,
    mut channels: [AnyAdcChannel<'static, ADC1>; 4],
    mut ntc_supply: AnyAdcChannel<'static, ADC1>,
    mut bus_sense: AnyAdcChannel<'static, ADC1>,
) {
    let mut supply_avg: MovingAverage<8> = MovingAverage::new();
    let mut avg: [MovingAverage<8>; 4] = [MovingAverage::new(), MovingAverage::new(), MovingAverage::new(), MovingAverage::new()];
    let mut ticker = Ticker::every(Duration::from_millis(250));
    let mut next = 0;
    let mut supply_samples = 0u8;
    loop {
        let raw = adc.blocking_read(&mut ntc_supply, SampleTime::CYCLES480);
        let measured = ntc::supply_volts(supply_avg.push(u32::from(raw)) as u16);
        // The average ramps up from zero (stock behaviour), which would read as cable breaks.
        supply_samples = supply_samples.saturating_add(1);
        let supply = if supply_samples >= 8 { measured } else { board::NTC_SUPPLY_NOMINAL };
        let raw = adc.blocking_read(&mut channels[next], SampleTime::CYCLES480);
        let mean = avg[next].push(u32::from(raw)) as u16;
        let reading = ntc::raw_to_reading(mean, supply);
        // For *Technical details › Board*: what the conversion saw, to check it on the unit.
        let mv = |v: f32| (v * 1000.0 + 0.5) as u16;
        crate::sysinfo::board_report(|b| {
            b.ntc_supply_mv = Some(mv(measured));
            b.ntc_raw.get_or_insert([0; 4])[next] = mean;
        });
        INPUTS.lock(|c| {
            let mut i = c.get();
            match next {
                0 => i.outdoor = reading,
                1 => i.supply = reading,
                2 => i.extract = reading,
                _ => i.exhaust = reading,
            }
            c.set(i);
        });
        next = (next + 1) % 4;
        // Module bus supply sense (PC2): stock converts it as V × 2.2 to mV.
        let raw = adc.blocking_read(&mut bus_sense, SampleTime::CYCLES480);
        let mv = u32::from(raw) * (board::ADC_VREF * 2.2 * 1000.0) as u32 / 4095;
        crate::bus::SENSE_MV.store(mv, Ordering::Relaxed);
        ticker.next().await;
    }
}

/// Measures the period between falling edges (stock: EXTI falling edge, no pull); the per-second tick in [`tacho_second`] drops
/// the speed to 0 when fewer than 3 pulses arrived (stock behaviour).
#[embassy_executor::task(pool_size = 2)]
pub async fn tacho_task(mut pin: ExtiInput<'static, Async>, rpm_out: &'static AtomicU32) {
    let mut t = tacho::Tacho::new();
    let mut last = Instant::now();
    let mut second = Instant::now();
    loop {
        // A stopped fan produces no edges: still tick once per second.
        let edge = embassy_futures::select::select(pin.wait_for_falling_edge(), Timer::at(second + Duration::from_secs(1))).await;
        let now = Instant::now();
        if let embassy_futures::select::Either::First(()) = edge {
            let period_us = (now - last).as_micros().min(u64::from(u32::MAX)) as u32;
            last = now;
            t.on_pulse(tacho::rpm_from_period_us(period_us, board::TACHO_PULSES_PER_REV));
        }
        if now >= second + Duration::from_secs(1) {
            second = now;
            t.on_second();
        }
        rpm_out.store(t.rpm(), Ordering::Relaxed);
    }
}

/// Fan PWM, fan enables and the bypass actuator.
pub struct Drive {
    pub pwm: SimplePwm<'static, TIM1>,
    pub exhaust_enable: Output<'static>,
    pub supply_enable: Output<'static>,
    pub bypass_open_pin: Output<'static>,
    pub bypass_close_pin: Output<'static>,
    bypass_run_s: u32,
    bypass_target: Option<bool>,
}

impl Drive {
    pub fn new(
        mut pwm: SimplePwm<'static, TIM1>,
        exhaust_enable: Output<'static>,
        supply_enable: Output<'static>,
        bypass_open_pin: Output<'static>,
        bypass_close_pin: Output<'static>,
    ) -> Self {
        for ch in [Channel::Ch1, Channel::Ch2, Channel::Ch3] {
            let mut c = pwm.channel(ch);
            c.set_duty_cycle_fully_off();
            c.enable();
        }
        Self { pwm, exhaust_enable, supply_enable, bypass_open_pin, bypass_close_pin, bypass_run_s: 0, bypass_target: None }
    }

    /// Safe state: fans off, bypass motor released (also used before a restart).
    pub fn all_off(&mut self) {
        self.pwm.channel(Channel::Ch1).set_duty_cycle_fully_off();
        self.pwm.channel(Channel::Ch2).set_duty_cycle_fully_off();
        self.exhaust_enable.set_low();
        self.supply_enable.set_low();
        self.bypass_open_pin.set_low();
        self.bypass_close_pin.set_low();
    }

    /// Applies one control-loop result; called once per second.
    pub fn apply(&mut self, o: &Outputs, actuator: ActuatorType, run_time_s: u16) {
        // CH1 = exhaust, CH2 = supply (inferred).
        self.pwm.channel(Channel::Ch1).set_duty_cycle_percent(o.exhaust_duty);
        self.pwm.channel(Channel::Ch2).set_duty_cycle_percent(o.supply_duty);
        self.exhaust_enable.set_level((o.exhaust_duty > 0).into());
        self.supply_enable.set_level((o.supply_duty > 0).into());

        // Bypass: drive for the configured run time after every change, as the stock firmware does.
        if self.bypass_target != Some(o.bypass_open) {
            self.bypass_target = Some(o.bypass_open);
            self.bypass_run_s = u32::from(run_time_s);
        }
        let running = self.bypass_run_s > 0;
        self.bypass_run_s = self.bypass_run_s.saturating_sub(1);
        match actuator {
            ActuatorType::Analog => {
                self.pwm.channel(Channel::Ch3).set_duty_cycle_percent(if o.bypass_open { 100 } else { 0 });
                self.bypass_open_pin.set_low();
                self.bypass_close_pin.set_low();
            }
            ActuatorType::Motor | ActuatorType::MotorReversed => {
                let (open, close) =
                    if actuator == ActuatorType::Motor { (o.bypass_open, !o.bypass_open) } else { (!o.bypass_open, o.bypass_open) };
                self.bypass_open_pin.set_level((running && open).into());
                self.bypass_close_pin.set_level((running && close).into());
            }
        }
    }
}

/// Status LEDs, active low (stock: PD6 1 Hz heartbeat, PD5 activity, PC7 low while the
/// Ethernet link is up).
pub struct Leds {
    pub heartbeat: Output<'static>,
    pub activity: Output<'static>,
    pub link: Output<'static>,
}

impl Leds {
    pub fn tick(&mut self, link_up: bool) {
        self.heartbeat.toggle();
        self.link.set_level(if link_up { Level::Low } else { Level::High });
    }

    pub fn off(&mut self) {
        self.heartbeat.set_high();
        self.activity.set_high();
        self.link.set_high();
    }
}

/// Raised when the mains zero-cross pulses on PB3 stop for [`POWER_FAIL_MS`].
pub static POWER_FAIL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Stock: every PB3 falling edge reloads a 50 ms countdown.
pub const POWER_FAIL_MS: u64 = 50;

/// Watches the zero-cross input. Like the stock firmware it only arms after the first pulse,
/// so a bench supply without mains pulses never reports a power failure.
///
/// Also counts the pulses per second for the *Board* details (50 or 100 on 50 Hz mains).
#[embassy_executor::task]
pub async fn power_fail_task(mut pin: ExtiInput<'static, Async>) {
    let mut second = Instant::now();
    let mut pulses: u16 = 0;
    let mut armed = false;
    loop {
        let edge = if armed {
            with_timeout(Duration::from_millis(POWER_FAIL_MS), pin.wait_for_falling_edge()).await.is_ok()
        } else {
            // Not armed yet: wait for the first pulse, but still report "0 per second".
            with_timeout(Duration::from_secs(1), pin.wait_for_falling_edge()).await.is_ok()
        };
        if edge {
            pulses = pulses.saturating_add(1);
            armed = true;
        } else if armed {
            POWER_FAIL.signal(());
            return;
        }
        if second.elapsed() >= Duration::from_secs(1) {
            let n = pulses;
            crate::sysinfo::board_report(|b| b.mains_pulses_per_s = Some(n));
            pulses = 0;
            second = Instant::now();
        }
    }
}
