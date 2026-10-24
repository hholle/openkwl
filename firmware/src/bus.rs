//! The module bus (RS-485 "Helios-BUS" on the RJ-10 socket) and the KWL-KNX on it, like the
//! stock firmware (protocol and reference frames in `openkwl-bus`):
//!
//! - **Power:** PE8 switches +24 V to the modules. Soft start (2 s at 500 Hz), then PE4 on.
//!   PC2 above 1000 mV for over a second = overcurrent: off for 5 s and again, three times;
//!   after the fourth trip it stays off (fault `bus_overcurrent`, stock Fehler 05).
//! - **Link:** USART1 (PA9 TX, PA10 RX) at 57600 8N1, PB10 = driver enable, high from the
//!   first byte until the last stop bit has left (transmit complete).
//! - **KNX:** `openkwl_bus::knx` decides what to send; this task only moves the frames.

use core::{
    cell::RefCell,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

use defmt::{debug, info, warn};
use embassy_stm32::{gpio::Output, pac};
use embassy_sync::blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex};
use embassy_time::{Duration, Instant, Timer};
use openkwl_api::{BusPower, KnxState, ModuleDetails};
use openkwl_bus::{
    knx::{self, Followup, Module, Presence},
    modbus::FRAME_MAX,
};

use crate::SharedApp;

/// Bus supply sense on PC2 in mV, from the sensor task (stock: ADC × 2.2).
pub static SENSE_MV: AtomicU32 = AtomicU32::new(0);
static POWER: AtomicU8 = AtomicU8::new(0);
/// What `Technical details` shows; copied into the app by the control loop.
static DETAILS: BlockingMutex<CriticalSectionRawMutex, RefCell<ModuleDetails>> = BlockingMutex::new(RefCell::new(ModuleDetails {
    bus_power: BusPower::Off,
    knx: KnxState::Searching,
    knx_firmware: None,
    knx_frames: 0,
    knx_errors: 0,
    knx_timeouts: 0,
    knx_rx_bytes: 0,
    knx_line_errors: 0,
    bus_sense_mv: 0,
}));

const OVER_MV: u32 = 1000;
const OVER_MS: u64 = 1000;
const OFF_AFTER_TRIP: Duration = Duration::from_secs(5);
/// Trips before the supply stays off (initial + 3 retries).
const TRIPS: u8 = 4;
/// The module boots after its supply comes on (stock polls it about 15 s later).
const MODULE_BOOT: Duration = Duration::from_secs(10);

pub fn details() -> ModuleDetails {
    let mut d = DETAILS.lock(|d| d.borrow().clone());
    d.bus_sense_mv = SENSE_MV.load(Ordering::Relaxed);
    d
}

fn set_power(p: BusPower) {
    POWER.store(p as u8, Ordering::Relaxed);
    DETAILS.lock(|d| d.borrow_mut().bus_power = p);
}

fn powered() -> bool {
    POWER.load(Ordering::Relaxed) == BusPower::On as u8
}

/// Bus supply: soft start, then watch for overcurrent.
#[embassy_executor::task]
pub async fn power_task(mut supply: Output<'static>, mut stage: Output<'static>) {
    let mut trips = 0;
    loop {
        set_power(BusPower::Starting);
        stage.set_low();
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            supply.set_low();
            Timer::after_millis(1).await;
            supply.set_high();
            Timer::after_millis(1).await;
        }
        supply.set_high();
        stage.set_high();
        Timer::after_secs(1).await;
        set_power(BusPower::On);
        debug!("bus: powered");

        // Above the limit for over a second (the sense value is updated every 250 ms).
        let mut over_since: Option<Instant> = None;
        loop {
            Timer::after_millis(50).await;
            if SENSE_MV.load(Ordering::Relaxed) > OVER_MV {
                if over_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_millis(OVER_MS) {
                    break;
                }
            } else {
                over_since = None;
            }
        }
        supply.set_low();
        stage.set_low();
        trips += 1;
        warn!("bus: overcurrent ({} of {})", trips, TRIPS);
        if trips >= TRIPS {
            set_power(BusPower::Overcurrent);
            // Stays off until the next restart (stock: until the fault is reset).
            core::future::pending::<()>().await;
        }
        set_power(BusPower::Off);
        Timer::after(OFF_AFTER_TRIP).await;
    }
}

/// USART1 on PA9 (TX) and PA10 (RX), 57600 8N1, set up with plain register writes: one
/// short frame a second needs no driver, and embassy's costs flash the slot does not have.
pub fn uart_init() {
    const RCC_APB2ENR: *mut u32 = 0x4002_3844 as *mut u32;
    const RCC_AHB1ENR: *mut u32 = 0x4002_3830 as *mut u32;
    const GPIOA_MODER: *mut u32 = 0x4002_0000 as *mut u32;
    const GPIOA_PUPDR: *mut u32 = 0x4002_000C as *mut u32;
    const GPIOA_AFRH: *mut u32 = 0x4002_0024 as *mut u32;
    // SAFETY: clock enables and the PA9/PA10 configuration fields at their fixed addresses;
    // nothing else uses USART1 or these pins.
    unsafe {
        let set = |reg: *mut u32, clear: u32, bits: u32| {
            reg.write_volatile(reg.read_volatile() & !clear | bits);
        };
        set(RCC_AHB1ENR, 0, 1 << 0); // GPIOA
        set(RCC_APB2ENR, 0, 1 << 4); // USART1
        set(GPIOA_AFRH, 0xFF << 4, 0x77 << 4); // PA9, PA10: AF7
        set(GPIOA_PUPDR, 0b11 << 20, 0); // PA10 no pull, as stock (the transceiver drives it)
        set(GPIOA_MODER, 0b1111 << 18, 0b1010 << 18); // alternate function
    }
    let r = pac::USART1;
    // 60 MHz APB2 (main.rs) / 57600 Bd = 1041.7, oversampling 16.
    r.brr().write_value(pac::usart::regs::Brr(1042));
    r.cr1().write(|w| {
        w.set_ue(true);
        w.set_te(true);
        w.set_re(true);
    });
}

/// What came back for a request.
struct Rx {
    /// Bytes kept in the answer buffer.
    len: usize,
    /// Bytes received with a framing, noise or overrun error.
    line_errors: u32,
}

/// Sends a request and collects the answer until the line is quiet (`len` 0 = nothing came
/// back). Blocking on purpose: it holds the normal executor for the frame plus at most the
/// answer timeout (50 ms) while no module answers.
fn transact(de: &mut Output<'static>, frame: &[u8], answer: &mut [u8]) -> Rx {
    let r = pac::USART1;
    // Drop anything left over (a late answer to an earlier request, other traffic).
    while r.sr().read().rxne() {
        let _ = r.dr().read();
    }
    // Receiver off while sending, so an echo of the own bytes cannot overrun it.
    r.cr1().modify(|w| w.set_re(false));
    de.set_high();
    for &b in frame {
        while !r.sr().read().txe() {}
        r.dr().write(|w| w.set_dr(u16::from(b)));
    }
    // The last stop bit must have left before the driver is released.
    while !r.sr().read().tc() {}
    de.set_low();
    r.cr1().modify(|w| w.set_re(true));
    let mut rx = Rx { len: 0, line_errors: 0 };
    let start = Instant::now();
    let deadline = start + Duration::from_millis(u64::from(knx::ANSWER_TIMEOUT_MS));
    // End of frame: 2 ms without a byte (stock: T3.5, 2 ticks of 1.45 ms) after the first one.
    let gap = Duration::from_millis(2);
    let mut last = start;
    loop {
        let sr = r.sr().read();
        if sr.rxne() || sr.ore() {
            let b = r.dr().read().dr() as u8; // reading DR also clears an overrun
            if sr.ore() || sr.fe() || sr.ne() {
                rx.line_errors += 1;
            }
            if rx.len < answer.len() {
                answer[rx.len] = b;
                rx.len += 1;
            }
            last = Instant::now();
        } else if (rx.len > 0 && last.elapsed() >= gap) || Instant::now() >= deadline {
            return rx;
        }
    }
}

/// Counts a reception in `Technical details`.
fn record(rx: &Rx) {
    DETAILS.lock(|d| {
        let mut d = d.borrow_mut();
        d.knx_rx_bytes = d.knx_rx_bytes.wrapping_add(rx.len as u32);
        d.knx_line_errors = d.knx_line_errors.wrapping_add(rx.line_errors);
    });
}

fn state(p: Presence) -> KnxState {
    match p {
        Presence::Searching => KnxState::Searching,
        Presence::Connected => KnxState::Connected,
        Presence::NotFound => KnxState::NotFound,
    }
}

fn publish(m: &Module) {
    DETAILS.lock(|d| {
        let mut d = d.borrow_mut();
        d.knx = state(m.presence);
        // "x.yy" by hand: core::fmt costs flash the debug build does not have.
        d.knx_firmware = (m.firmware > 0).then(|| {
            let (whole, frac) = (m.firmware / 100, m.firmware % 100);
            let mut s = heapless::String::new();
            let mut div = 1;
            while whole / div >= 10 {
                div *= 10;
            }
            while div > 0 {
                let _ = s.push((b'0' + (whole / div % 10) as u8) as char);
                div /= 10;
            }
            for d in [b'.', b'0' + (frac / 10) as u8, b'0' + (frac % 10) as u8] {
                let _ = s.push(d as char);
            }
            s
        });
        d.knx_frames = m.good;
        d.knx_errors = m.bad;
        d.knx_timeouts = m.missed;
    });
}

/// Polls the KWL-KNX once a second while the bus is powered.
#[embassy_executor::task]
pub async fn knx_task(app: &'static SharedApp, mut de: Output<'static>) {
    let mut m = Module::new();
    let mut frame = [0u8; FRAME_MAX];
    let mut answer = [0u8; FRAME_MAX];
    loop {
        while !powered() {
            Timer::after_millis(500).await;
        }
        Timer::after(MODULE_BOOT).await;
        while powered() {
            let n = Module::request(&mut frame);
            let rx = transact(&mut de, &frame[..n], &mut answer);
            record(&rx);
            if rx.len == 0 {
                m.on_timeout();
            } else {
                let s = app.lock().await.knx_snapshot();
                if let Ok((cmd, follow)) = m.on_answer(&answer[..rx.len], &s, &mut frame) {
                    if let Some(c) = cmd {
                        app.lock().await.knx_command(c);
                    }
                    if let Followup::Write { len, .. } | Followup::Read { len, .. } = follow {
                        let rx = transact(&mut de, &frame[..len], &mut answer);
                        record(&rx);
                        if rx.len == 0 {
                            m.on_timeout();
                        } else {
                            if let Ok(Some(push)) = m.on_followup_answer(&answer[..rx.len], follow) {
                                app.lock().await.knx_push(push);
                            }
                        }
                    }
                }
            }
            let was = DETAILS.lock(|d| d.borrow().knx);
            publish(&m);
            if was != state(m.presence) {
                match (was, m.presence) {
                    (_, Presence::Connected) => {
                        info!("bus: KNX module found, firmware {}", m.firmware);
                        app.lock().await.knx_found(m.firmware);
                    }
                    (KnxState::Connected, _) => app.lock().await.knx_log("module lost"),
                    (_, Presence::NotFound) => app.lock().await.knx_log("no module answers, asking every 10 s"),
                    _ => {}
                }
            }
            Timer::after_millis(u64::from(m.next_poll_ms())).await;
        }
    }
}
