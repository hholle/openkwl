//! Facts about the chip and the running image for `GET /system/details`: reset cause, device
//! and revision ID, unique ID, flash size, image and RAM layout, stack high-water mark.

use core::{
    cell::RefCell,
    ptr::{addr_of, read_volatile},
};

use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

use openkwl_api::{
    ResetReason,
    system::{ChipDetails, FirmwareDetails, MemoryDetails},
};

const RCC_CSR: *mut u32 = 0x4002_3874 as *mut u32;
const DBGMCU_IDCODE: *const u32 = 0xE004_2000 as *const u32;
const FLASH_SIZE_KB: *const u16 = 0x1FFF_7A22 as *const u16;
const APP_START: u32 = 0x0800_8000;
const STACK_PAINT: u32 = 0xCCCC_CCCC;

unsafe extern "C" {
    static __sidata: u32;
    static __sdata: u32;
    static __edata: u32;
    static __ebss: u32;
    static _stack_end: u32;
    static _stack_start: u32;
}

fn addr(sym: *const u32) -> u32 {
    sym as u32
}

/// Reads and clears the reset flags. Call once, early.
/// RCC_CSR bits 31..24 of this start, for the boot record.
pub static RESET_FLAGS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn take_reset_cause() -> (ResetReason, u8) {
    // SAFETY: RCC_CSR is a valid, always-readable register; writing RMVF only clears the flags.
    let csr = unsafe { read_volatile(RCC_CSR) };
    unsafe { RCC_CSR.write_volatile(csr | 1 << 24) };
    let bit = |n: u32| csr & 1 << n != 0;
    let reason = if bit(29) || bit(30) {
        ResetReason::Watchdog
    } else if bit(28) {
        ResetReason::Software
    } else if bit(27) {
        ResetReason::PowerOn
    } else if bit(25) {
        ResetReason::Brownout
    } else if bit(26) {
        ResetReason::Pin
    } else {
        ResetReason::Unknown
    };
    RESET_FLAGS.store((csr >> 24) as u8, core::sync::atomic::Ordering::Relaxed);
    (reason, (csr >> 24) as u8)
}

pub fn chip(reset: (ResetReason, u8), sysclk_hz: u32, hse_hz: Option<u32>) -> ChipDetails {
    // SAFETY: fixed system-memory and debug addresses of the STM32F2, always readable.
    let id = unsafe { read_volatile(DBGMCU_IDCODE) };
    let flash_kb = unsafe { read_volatile(FLASH_SIZE_KB) };
    let device_id = (id & 0xFFF) as u16;
    let mut unique_id = heapless::String::new();
    for b in embassy_stm32::uid::uid() {
        let _ = core::fmt::write(&mut unique_id, format_args!("{b:02x}"));
    }
    let mut mcu = heapless::String::new();
    let family = if device_id == 0x411 || device_id == 0 { "STM32F2" } else { "STM32" };
    let _ = core::fmt::write(&mut mcu, format_args!("{family} {flash_kb}K"));
    ChipDetails {
        mcu,
        device_id,
        revision_id: (id >> 16) as u16,
        unique_id,
        flash_kb,
        sysclk_hz,
        hse_hz,
        reset_reason: reset.0,
        reset_flags: reset.1,
    }
}

pub fn firmware() -> FirmwareDetails {
    fn s<const N: usize>(v: &str) -> heapless::String<N> {
        v.try_into().unwrap_or_default()
    }
    FirmwareDetails {
        version: s(env!("CARGO_PKG_VERSION")),
        build: s(option_env!("OPENKWL_BUILD").unwrap_or("dev")),
        platform: s("stm32f2"),
        target: s("thumbv7m-none-eabi"),
        rustc: s(env!("OPENKWL_RUSTC")),
        profile: s(if cfg!(debug_assertions) { "debug" } else { "release" }),
        built_unix_s: env!("OPENKWL_BUILT").parse().ok(),
        uptime_s: 0,
    }
}

pub fn memory() -> MemoryDetails {
    // Only the addresses of the linker symbols are taken.
    let (sidata, sdata, edata, ebss, stack_end, stack_start) = (
        addr(addr_of!(__sidata)),
        addr(addr_of!(__sdata)),
        addr(addr_of!(__edata)),
        addr(addr_of!(__ebss)),
        addr(addr_of!(_stack_end)),
        addr(addr_of!(_stack_start)),
    );
    MemoryDetails {
        flash_slot_bytes: crate::board::FLASH_SLOT_BYTES,
        image_bytes: sidata - APP_START + (edata - sdata),
        ram_bytes: crate::board::RAM_BYTES,
        static_ram_bytes: ebss - sdata,
        stack_bytes: stack_start - stack_end,
        stack_peak_bytes: None,
    }
}

/// Deepest stack use since boot, from the paint left by cortex-m-rt (`paint-stack`).
pub fn stack_peak_bytes() -> u32 {
    // SAFETY: reads RAM between the linker's stack bounds, word-aligned.
    unsafe {
        let (end, start) = (addr(addr_of!(_stack_end)), addr(addr_of!(_stack_start)));
        let mut p = end;
        while p < start && read_volatile(p as *const u32) == STACK_PAINT {
            p += 4;
        }
        start - p
    }
}

/// Raw board readings collected at boot and while running (`GET /system/details`, *Board*).
static BOARD: Mutex<CriticalSectionRawMutex, RefCell<openkwl_api::BoardDetails>> =
    Mutex::new(RefCell::new(openkwl_api::BoardDetails {
        card_detect_present: None,
        sd_power_active_low: None,
        mains_pulses_per_s: None,
        phy_address: None,
        ntc_supply_mv: None,
        ntc_supply_used_mv: None,
        ntc_raw: None,
    }));

pub fn board_report(f: impl FnOnce(&mut openkwl_api::BoardDetails)) {
    BOARD.lock(|b| f(&mut b.borrow_mut()));
}

pub fn board() -> openkwl_api::BoardDetails {
    BOARD.lock(|b| b.borrow().clone())
}
