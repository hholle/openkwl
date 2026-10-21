//! Board facts in one place, determined for interoperability from how the stock 2.24/2.27
//! firmware and the bootloader drive the board. "Observed" = the stock firmware configures the
//! pin this way; "inferred" = role deduced from how the stock firmware uses it; both still want a
//! look on the bench before the first install (README bring-up checklist).
//!
//! | Function | Pin / setting | Source |
//! |---|---|---|
//! | Exhaust fan (CH1) / supply fan (CH2) 0–10 V | PE9 / PE11, TIM1_CH1/2 | observed |
//! | Bypass analog actuator | PE13 TIM1_CH3 (PE14 CH4 is set up but never written) | observed |
//! | Fan enables CH1 / CH2 | PE2 / PE3 | observed |
//! | Tacho supply (CH2) / exhaust (CH1) | PB8 / PB9, EXTI falling edge, no pull; **1 pulse/rev** | observed (timers), inferred (pulses) |
//! | Bypass motor open / close | PD10 / PD11 (PD8/PD9 driven low with them) | observed |
//! | T1 outdoor / T2 supply / T3 extract / T4 exhaust | PA5 / PA6 / PB0 / PB1 (ADC1 IN5/6/8/9), Vref 2.5 V | observed |
//! | NTC supply reference | PC3 (ADC1 IN13), V × 2, used for the NTC conversion | observed |
//! | Supply rail | PA4 (ADC1 IN4), V × 23, ~24 V | observed (scale), inferred (role) |
//! | 0–10 V inputs | PA0 (level switch, 2/4/6/8 V), PA3 (4.6 V threshold) | observed (scale), inferred (role) |
//! | SD card | SDIO PC8–PC12, PD2; power PA15 (active low); no card detect (the stock firmware reads PH13, which the 100-pin STM32F207VC lacks) | observed (pins), inferred (polarity) |
//! | Ethernet | RMII: PA1 REF_CLK (external 50 MHz), PA2 MDIO, PC1 MDC, PA7 CRS_DV, PC4/PC5 RXD0/1, PB11 TX_EN, PB12/PB13 TXD0/1 | observed |
//! | Ethernet PHY | Micrel KSZ8031/8051RNL (0x0022/0x155x) or KSZ8081RNA/RND (0x156x, 2.27); address scanned; reset PD15 (active low) | observed |
//! | Clock | HSE **25 MHz** crystal; stock app 100 MHz, bootloader 120 MHz | observed |
//! | RTC | LSE 32.768 kHz crystal; BKP1 = bootloader version | observed |
//! | Power fail | PB3 = mains zero-cross pulses (EXTI3 falling); 50 ms without a pulse = power failing | observed (code), inferred (zero-cross) |
//! | Status LEDs | PD6 heartbeat, PD5 activity, PC7 Ethernet link; active low | inferred |
//! | Electric pre-heater (VHZ EH) | PB14 SSR, zero-cross burst firing; held **off** (owner has none) | inferred |
//! | Module bus (BEC panel, sensors) | PE8 supply switch (+PE4), PC2 current sense, USART1 PA9/PA10 + PB10 DE; held **off** | inferred |
//! | Fault outputs | PE5 / PE6; held low | inferred |
//! | Humidity sensor (SHT1x/7x, optional) | PB6 SCK / PB7 DATA, bit-banged | inferred |
//! | Factory MAC | OTP 0x1FFF7800, ASCII | observed |
//! | Internal flash | sector 0 bootloader, **sector 1 stock settings ring buffer (keep)**, sectors 2–5 app | observed |

use core::sync::atomic::{AtomicU32, Ordering};

/// PWM frequency for the 0–10 V outputs: stock TIM1 PSC 72, ARR 99 at 100 MHz = 13.7 kHz,
/// matching the board's output filters.
pub const PWM_HZ: u32 = 13_700;
/// Tacho pulses per fan revolution: the stock `3 156 000 / ticks` on a 52.576 kHz tick is
/// exactly 60 × f / ticks, i.e. one pulse per revolution.
pub const TACHO_PULSES_PER_REV: u32 = 1;
/// ADC reference, and the NTC pull-up supply until PC3 has a plausible reading.
pub const ADC_VREF: f32 = 2.5;
/// PC3 readings outside this range (a broken divider or pin) are ignored and `ADC_VREF` is
/// used instead; the stock firmware takes the reading as it is.
pub const NTC_SUPPLY_VOLTS: core::ops::RangeInclusive<f32> = 2.2..=2.8;
/// Independent watchdog timeout (stock: 10 s).
pub const WATCHDOG_US: u32 = 8_000_000;
/// External crystal.
pub const HSE_HZ_VALUE: u32 = 25_000_000;
pub const HSE_HZ: Option<u32> = Some(HSE_HZ_VALUE);
/// System clock set up in `main.rs::clocks`.
pub const SYSCLK_HZ: u32 = 120_000_000;
/// RTC clock source.
pub const RTC_CLOCK: &str = "LSE";
/// Application slot behind the stock bootloader (`memory.x`, the stock image size).
pub const FLASH_SLOT_BYTES: u32 = 0x37E00;
/// STM32F207VCT6 (100 pins, 256 KB flash, 128 KB RAM), read off the chip.
pub const RAM_BYTES: u32 = 128 * 1024;

/// CRC of the running image: the bootloader flashes the file's trailer along, so it sits right
/// after the 0x37E00-byte body. Pairs the firmware with its web UI (`openkwl_update::MARKER`).
pub fn image_crc() -> u16 {
    // SAFETY: inside the application slot, always readable.
    unsafe { core::ptr::read_volatile((0x0800_8000 + FLASH_SLOT_BYTES) as *const u16) }
}

/// RTC backup register 1: the stock bootloader writes its version there (110 = 1.10).
const RTC_BKP1R: *const u32 = 0x4000_2854 as *const u32;
static BOOTLOADER: AtomicU32 = AtomicU32::new(0);

/// Call once after the RTC (and with it the backup domain) is enabled.
pub fn read_bootloader_version() {
    // SAFETY: backup register read; the RTC driver has enabled the PWR/backup-domain access.
    BOOTLOADER.store(unsafe { core::ptr::read_volatile(RTC_BKP1R) }, Ordering::Relaxed);
}

/// Version of the stock bootloader, e.g. `1.10`; empty when not recognised.
pub fn bootloader_version() -> heapless::String<16> {
    let mut s = heapless::String::new();
    let v = BOOTLOADER.load(Ordering::Relaxed);
    if (100..1000).contains(&v) {
        let _ = core::fmt::write(&mut s, format_args!("{}.{:02}", v / 100, v % 100));
    }
    s
}
