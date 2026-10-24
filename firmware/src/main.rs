//! openKWL: the hardware layer around `openkwl-app` for the Helios easyControls main
//! controller (STM32F2x7). All device behaviour (control loop, REST API, persistence
//! decisions) is in `openkwl-app` and tested on the host with `tools/openkwl-sim`; this crate
//! wires it to the peripherals.
//!
//! **Never run on a unit yet**: details marked "inferred" in [`board`] are checked in the first
//! test (`docs/PLAN.md` §2, `docs/INSTALL.md`).

#![no_std]
#![no_main]

mod board;
mod bus;
mod control;
mod early;
mod http;
mod io;
mod mdns;
mod mqtt;
mod net;
#[cfg(not(any(feature = "rtt", feature = "sd-log")))]
mod nolog;
mod phy;
#[cfg(feature = "sd-log")]
mod sdlog;
mod sntp;
mod storage;
mod sysinfo;
#[cfg(feature = "update")]
mod update;

use defmt::info;
use embassy_executor::{InterruptExecutor, Spawner};
use embassy_stm32::{
    Config,
    adc::{Adc, AdcChannel},
    bind_interrupts, eth, exti,
    gpio::{Level, Output, OutputType, Pull, Speed},
    interrupt::{self, InterruptExt, Priority},
    peripherals, rcc, rng,
    rtc::{Rtc, RtcConfig},
    sdmmc,
    time::Hertz,
    timer::{
        low_level::CountingMode,
        simple_pwm::{PwmPin, SimplePwm},
    },
    wdg::IndependentWatchdog,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use openkwl_app::{App, PlatformInfo};
use static_cell::StaticCell;
#[cfg(feature = "rtt")]
use {defmt_rtt as _, panic_probe as _};

/// A task's spawn token. A full task pool is a programming error that shows on the first
/// start; no message, it would only cost flash.
fn task<S>(token: Result<S, embassy_executor::SpawnError>) -> S {
    token.unwrap_or_else(|_| panic!())
}

bind_interrupts!(pub struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
    ETH => eth::InterruptHandler;
    SDIO => sdmmc::InterruptHandler<peripherals::SDIO>;
    DMA2_STREAM3 => embassy_stm32::dma::InterruptHandler<peripherals::DMA2_CH3>;
    EXTI3 => exti::InterruptHandler<interrupt::typelevel::EXTI3>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

/// Higher-priority executor for the control loop (runs in the UART4 interrupt, which is
/// otherwise unused on this board).
static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();

#[embassy_stm32::interrupt]
unsafe fn UART4() {
    // SAFETY: the executor's interrupt handler, as required by `InterruptExecutor::start`.
    unsafe { EXECUTOR_HIGH.on_interrupt() }
}

/// The device application, shared by the control loop, HTTP and SNTP.
pub type SharedApp = Mutex<CriticalSectionRawMutex, App>;
/// Hardware RNG for session tokens, password salts and SNTP nonces.
pub type SharedRng = Mutex<CriticalSectionRawMutex, rng::Rng<'static, peripherals::RNG>>;

/// `hse`: the 25 MHz crystal runs; otherwise the PLL runs from the internal 16 MHz
/// oscillator (as the stock firmware falls back to it), same 120 MHz.
/// `lse`: the 32.768 kHz crystal runs (`early::lse_starts`); otherwise the RTC runs from the
/// internal ~32 kHz LSI (less accurate, SNTP corrects it).
fn clocks(hse: bool, lse: bool) -> Config {
    let mut c = Config::default();
    // HSE 25 MHz / 25 × 240 / 2 = 120 MHz SYSCLK, Q = 48 MHz: the stock bootloader's PLL
    // (the stock app runs the same crystal at 100 MHz). The bootloader resets RCC before the
    // jump, so we start on HSI.
    if hse {
        c.rcc.hse = Some(rcc::Hse { freq: Hertz(board::HSE_HZ_VALUE), mode: rcc::HseMode::Oscillator });
        c.rcc.pll_src = rcc::PllSource::HSE;
    } else {
        c.rcc.hsi = true;
        c.rcc.pll_src = rcc::PllSource::HSI;
    }
    c.rcc.pll = Some(rcc::Pll {
        prediv: if hse { rcc::PllPreDiv::DIV25 } else { rcc::PllPreDiv::DIV16 },
        mul: rcc::PllMul::MUL240,
        divp: Some(rcc::PllPDiv::DIV2),
        divq: Some(rcc::PllQDiv::DIV5),
        divr: None,
    });
    c.rcc.sys = rcc::Sysclk::PLL1_P;
    c.rcc.ahb_pre = rcc::AHBPrescaler::DIV1;
    c.rcc.apb1_pre = rcc::APBPrescaler::DIV4;
    c.rcc.apb2_pre = rcc::APBPrescaler::DIV2;
    // 32.768 kHz crystal, as the stock firmware (a different RTC source resets the backup
    // domain and with it the stock bootloader's registers: only when the crystal is dead).
    c.rcc.ls = if lse { rcc::LsConfig::default_lse() } else { rcc::LsConfig::default_lsi() };
    c
}

fn platform(reset: (openkwl_api::ResetReason, u8), hse: bool, lse: bool) -> PlatformInfo {
    PlatformInfo {
        firmware: sysinfo::firmware(),
        chip: sysinfo::chip(reset, board::SYSCLK_HZ, board::HSE_HZ.filter(|_| hse)),
        memory: sysinfo::memory(),
        // Filled from the card by `storage::boot`.
        stock: Default::default(),
        rtc_clock: (if lse { board::RTC_CLOCK } else { "LSI" }).try_into().unwrap_or_default(),
    }
}

/// Builds the 8 KB `App` in its static, outside `main`'s async frame (see `storage::place_card`).
#[inline(never)]
fn place_app(boot: openkwl_app::Boot) -> &'static SharedApp {
    static APP: StaticCell<SharedApp> = StaticCell::new();
    APP.init_with(|| Mutex::new(App::new(boot)))
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Interrupts are off since `__pre_init` (early.rs); clear what the bootloader left on.
    early::quiet_bootloader();
    // SAFETY: no interrupt is pending any more; the drivers enable their own lines.
    unsafe { cortex_m::interrupt::enable() };
    early::start_watchdog();
    let reset = sysinfo::take_reset_cause();
    let hse = early::hse_starts();
    let lse = early::lse_starts();
    if !lse {
        // Switching the RTC to LSI resets the backup domain: keep the bootloader version.
        board::read_bootloader_version();
    }
    let p = embassy_stm32::init(clocks(hse, lse));
    // Stage: clocks up (PD5 only; both on = stopped before here).
    early::leds(true, false);
    info!("openKWL {} starting, reset flags {=u8:#x} (RCC_CSR 31..24)", env!("CARGO_PKG_VERSION"), reset.1);

    let mut watchdog = IndependentWatchdog::new(p.IWDG, board::WATCHDOG_US);
    watchdog.unleash();

    // Outputs first, in their safe state (fans off, bypass motor released, heater and module
    // bus off, LEDs off). Pins this firmware never changes again are left driven.
    for pin in [
        Output::new(p.PB14, Level::Low, Speed::Low),
        Output::new(p.PE5, Level::Low, Speed::Low),
        Output::new(p.PE6, Level::Low, Speed::Low),
        Output::new(p.PD8, Level::Low, Speed::Low),
        Output::new(p.PD9, Level::Low, Speed::Low),
    ] {
        core::mem::forget(pin);
    }
    // Start-up stages on the LEDs (firmware/README.md): from here PD5 (activity) stays on and
    // PD6 (heartbeat) off until the control loop runs and blinks it; activity goes off if no
    // SD card answers.
    let mut leds = io::Leds {
        heartbeat: Output::new(p.PD6, Level::High, Speed::Low),
        activity: Output::new(p.PD5, Level::Low, Speed::Low),
        link: Output::new(p.PC7, Level::High, Speed::Low),
    };
    let pwm = SimplePwm::new(
        p.TIM1,
        Some(PwmPin::new(p.PE9, OutputType::PushPull)),
        Some(PwmPin::new(p.PE11, OutputType::PushPull)),
        Some(PwmPin::new(p.PE13, OutputType::PushPull)),
        None,
        Hertz(board::PWM_HZ),
        CountingMode::EdgeAlignedUp,
    );
    let drive = io::Drive::new(
        pwm,
        Output::new(p.PE2, Level::Low, Speed::Low),
        Output::new(p.PE3, Level::Low, Speed::Low),
        Output::new(p.PD10, Level::Low, Speed::Low),
        Output::new(p.PD11, Level::Low, Speed::Low),
    );

    static RNG: StaticCell<SharedRng> = StaticCell::new();
    let rng: &'static SharedRng = RNG.init(Mutex::new(rng::Rng::new(p.RNG, Irqs)));
    let (rtc, rtc_time) = Rtc::new(p.RTC, RtcConfig::default());
    if lse {
        board::read_bootloader_version();
    }

    // SD card: settings, counters, web UI.
    let card = storage::init(p.SDIO, p.DMA2_CH3, Irqs, p.PC12, p.PD2, p.PC8, p.PC9, p.PC10, p.PC11, p.PA15, &mut watchdog).await;
    if !storage::PRESENT.load(core::sync::atomic::Ordering::Relaxed) {
        leds.activity.set_high();
    }
    // Card start-up can take a few seconds (two power polarities, slow cards); the control
    // loop, which feeds the watchdog from then on, starts only after the settings are read.
    watchdog.pet();
    let boot = storage::boot(card, platform(reset, hse, lse)).await;
    watchdog.pet();
    let mac = storage::mac_address(&boot.config);
    let network = boot.config.network.clone();
    let app = place_app(boot);
    leds.link.set_low();
    // Saves (and the debug log) work from here on, even if the network start-up hangs.
    spawner.spawn(crate::task(storage::storage_task(app, card)));

    // Sensors and tachometers.
    let adc = Adc::new(p.ADC1);
    let channels = [p.PA5.degrade_adc(), p.PA6.degrade_adc(), p.PB0.degrade_adc(), p.PB1.degrade_adc()];
    spawner.spawn(crate::task(io::sensor_task(adc, channels, p.PC3.degrade_adc(), p.PC2.degrade_adc())));
    let supply_tacho = exti::ExtiInput::new(p.PB8, p.EXTI8, Pull::None, Irqs);
    let exhaust_tacho = exti::ExtiInput::new(p.PB9, p.EXTI9, Pull::None, Irqs);
    // Mains zero-cross pulses; 50 ms without one = power failing.
    let zero_cross = exti::ExtiInput::new(p.PB3, p.EXTI3, Pull::None, Irqs);

    // The control loop gets its own executor one priority above everything else (SD card,
    // network, HTTP), so neither can delay the fans or the watchdog. The tachometers and the
    // power-fail watch run there too: they time edges, and an SD write blocks the normal
    // executor for up to a few hundred milliseconds.
    spawner.spawn(crate::task(control::alive_task()));
    interrupt::UART4.set_priority(Priority::P6);
    let high = EXECUTOR_HIGH.start(interrupt::UART4);
    high.spawn(crate::task(io::tacho_task(supply_tacho, &io::SUPPLY_RPM)));
    high.spawn(crate::task(io::tacho_task(exhaust_tacho, &io::EXHAUST_RPM)));
    high.spawn(crate::task(io::power_fail_task(zero_cross)));
    high.spawn(crate::task(control::control_task(app, drive, leds, rtc, rtc_time, watchdog)));

    // Network: Ethernet, DHCP/static, HTTP, SNTP.
    let pins = net::Pins {
        eth: p.ETH,
        ref_clk: p.PA1,
        crs: p.PA7,
        rx_d0: p.PC4,
        rx_d1: p.PC5,
        tx_d0: p.PB12,
        tx_d1: p.PB13,
        tx_en: p.PB11,
        sma: p.ETH_SMA,
        mdio: p.PA2,
        mdc: p.PC1,
        phy_reset: p.PD15,
    };
    // Without the RMII clock the unit runs on without a network (ventilation, saves, module bus).
    let stack = net::start(&spawner, pins, mac, &network, &mut *rng.lock().await).await;
    if let Some(stack) = stack {
        for id in 0..http::WORKERS {
            spawner.spawn(crate::task(http::server_task(id, stack, app, rng, card)));
        }
        spawner.spawn(crate::task(sntp::sntp_task(stack, app, rng)));
        spawner.spawn(crate::task(mqtt::mqtt_task(stack, app)));
        spawner.spawn(crate::task(mdns::mdns_task(stack, app)));
    }

    // Module bus: +24 V for extension modules (PE8, PE4), RS-485 on USART1 with PB10 as
    // driver enable, and the KWL-KNX on it.
    let supply = Output::new(p.PE8, Level::Low, Speed::Low);
    let stage = Output::new(p.PE4, Level::Low, Speed::Low);
    spawner.spawn(crate::task(bus::power_task(supply, stage)));
    let de = Output::new(p.PB10, Level::Low, Speed::Low);
    bus::uart_init();
    spawner.spawn(crate::task(bus::knx_task(app, de)));
}
