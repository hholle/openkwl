//! The first steps after the bootloader's jump, before `embassy_stm32::init`.
//!
//! - The bootloader leaves the SDIO and DMA2 stream 3 interrupts enabled (it read the card
//!   with them) and interrupts globally on. Our handlers for those lines are bound, so a
//!   pending or late interrupt would run a driver that is not set up yet (embassy's DMA
//!   handler panics on a transfer-error flag). All NVIC lines are switched off and cleared,
//!   and DMA2 and SDIO are reset; the drivers enable what they use.
//! - The watchdog starts right away with plain register writes, so a hang while the clocks
//!   start (crystal, PLL, LSE for the RTC) becomes a restart loop instead of a dead unit.

use core::ptr::write_volatile;

const RCC_AHB1RSTR: *mut u32 = 0x4002_3810 as *mut u32;
const RCC_APB2RSTR: *mut u32 = 0x4002_3824 as *mut u32;
const DMA2RST: u32 = 1 << 22;
const SDIORST: u32 = 1 << 11;

const IWDG_KR: *mut u32 = 0x4000_3000 as *mut u32;
const IWDG_PR: *mut u32 = 0x4000_3004 as *mut u32;
const IWDG_RLR: *mut u32 = 0x4000_3008 as *mut u32;
/// DBGMCU_APB1_FZ: DBG_IWDG_STOP, the watchdog pauses while a probe halts the core.
const DBGMCU_APB1_FZ: *mut u32 = 0xE004_2008 as *mut u32;

/// Switches off what the bootloader left running. Runs before RAM is initialised (only
/// registers, no statics); interrupts stay off until `main` turns them on.
pub fn quiet_bootloader() {
    cortex_m::interrupt::disable();
    // SAFETY: NVIC, RCC reset registers: fixed system addresses; nothing else runs yet.
    unsafe {
        let nvic = &*cortex_m::peripheral::NVIC::PTR;
        for i in 0..nvic.icer.len() {
            nvic.icer[i].write(u32::MAX);
            nvic.icpr[i].write(u32::MAX);
        }
        for (reg, bits) in [(RCC_AHB1RSTR, DMA2RST), (RCC_APB2RSTR, SDIORST)] {
            let v = core::ptr::read_volatile(reg);
            write_volatile(reg, v | bits);
            write_volatile(reg, v & !bits);
        }
    }
}

/// Starts the 25 MHz crystal and waits up to ~10 ms, like the stock firmware (which waits
/// even less and then stays on the internal oscillator). `false`: run the PLL from HSI.
pub fn hse_starts() -> bool {
    const RCC_CR: *mut u32 = 0x4002_3800 as *mut u32;
    const HSEON: u32 = 1 << 16;
    const HSERDY: u32 = 1 << 17;
    // SAFETY: RCC_CR at its fixed address; only the HSE bits change.
    unsafe {
        write_volatile(RCC_CR, core::ptr::read_volatile(RCC_CR) | HSEON);
        for _ in 0..40_000 {
            if core::ptr::read_volatile(RCC_CR) & HSERDY != 0 {
                return true;
            }
        }
        write_volatile(RCC_CR, core::ptr::read_volatile(RCC_CR) & !HSEON);
    }
    false
}

/// Starts the 32.768 kHz crystal for the RTC (or finds it running: the backup domain keeps it
/// across resets) and waits up to about 3 s for it (`embassy_stm32::init` would wait without
/// a limit, a boot loop with a dead crystal). `false`: the RTC runs from LSI instead.
pub fn lse_starts() -> bool {
    const RCC_APB1ENR: *mut u32 = 0x4002_3840 as *mut u32;
    const PWREN: u32 = 1 << 28;
    const PWR_CR: *mut u32 = 0x4000_7000 as *mut u32;
    const DBP: u32 = 1 << 8;
    const RCC_BDCR: *mut u32 = 0x4002_3870 as *mut u32;
    const LSEON: u32 = 1 << 0;
    const LSERDY: u32 = 1 << 1;
    // SAFETY: RCC, PWR registers at their fixed addresses; backup-domain write access is
    // enabled as embassy does it, and only LSEON is set (a no-op when the crystal already runs).
    unsafe {
        let ready = || core::ptr::read_volatile(RCC_BDCR) & LSERDY != 0;
        if ready() {
            return true;
        }
        write_volatile(RCC_APB1ENR, core::ptr::read_volatile(RCC_APB1ENR) | PWREN);
        write_volatile(PWR_CR, core::ptr::read_volatile(PWR_CR) | DBP);
        write_volatile(RCC_BDCR, core::ptr::read_volatile(RCC_BDCR) | LSEON);
        // ~250 ns per round on the 16 MHz HSI (see `hse_starts`); the crystal typically needs
        // up to 2 s. Well inside the 10 s of the watchdog started before.
        if (0..12_000_000).any(|_| ready()) {
            return true;
        }
        // Off again, so the LSI setup finds the backup domain as it left it and does not
        // reset it (with the RTC time) on every start.
        write_volatile(RCC_BDCR, core::ptr::read_volatile(RCC_BDCR) & !LSEON);
    }
    false
}

/// Starts the independent watchdog at 10 s (÷256 at ~32 kHz LSI, reload 1250), like the
/// stock firmware; `IndependentWatchdog` takes it over later with its own timeout.
pub fn start_watchdog() {
    // SAFETY: IWDG and DBGMCU registers at their fixed addresses; the key sequence is the
    // documented one (enable write access, set prescaler and reload, reload, start).
    unsafe {
        write_volatile(DBGMCU_APB1_FZ, core::ptr::read_volatile(DBGMCU_APB1_FZ) | 1 << 12);
        write_volatile(IWDG_KR, 0x5555);
        write_volatile(IWDG_PR, 6);
        write_volatile(IWDG_RLR, 1250);
        write_volatile(IWDG_KR, 0xAAAA);
        write_volatile(IWDG_KR, 0xCCCC);
    }
}

/// Start-up stage on the two LEDs (active low, PD5 and PD6), set with plain register writes so
/// it works before the drivers: `pd5`/`pd6` = on. See firmware/README.md, *Debugging*.
pub fn leds(pd5: bool, pd6: bool) {
    const GPIOD_BSRR: *mut u32 = 0x4002_0C18 as *mut u32;
    let bit = |on: bool, pin: u32| if on { 1 << (pin + 16) } else { 1 << pin };
    // SAFETY: GPIOD set/reset register; `__pre_init` made PD5 and PD6 outputs.
    unsafe { write_volatile(GPIOD_BSRR, bit(pd5, 5) | bit(pd6, 6)) };
}

// The very first code after the bootloader's jump, before RAM is initialised. Plain
// assembler, no stack:
// - interrupts off (the bootloader jumps with them on and the SDIO/DMA lines enabled), so no
//   handler runs on uninitialised statics; `main` clears the NVIC and turns them back on;
// - PD6 on = our image runs; then a write/read test at the top of RAM, where the stack is
//   (a fault there locks the core with only PD6 on); passed = PD5 on too; a wrong read-back
//   blinks both fast forever.
core::arch::global_asm!(
    ".section .text.__pre_init,\"ax\"",
    ".global __pre_init",
    ".type __pre_init,%function",
    ".thumb_func",
    "__pre_init:",
    "    cpsid i",
    "    ldr r0, =0x40023830", // RCC_AHB1ENR: GPIOD clock
    "    ldr r1, [r0]",
    "    orr r1, r1, #8",
    "    str r1, [r0]",
    "    ldr r0, =0x40020C00", // GPIOD_MODER: PD5, PD6 outputs
    "    ldr r1, [r0]",
    "    bic r1, r1, #0x3C00",
    "    orr r1, r1, #0x1400",
    "    str r1, [r0]",
    "    ldr r0, =0x40020C18", // GPIOD_BSRR
    "    ldr r1, =0x00400020", // PD6 on (low), PD5 off (high)
    "    str r1, [r0]",
    "    ldr r1, =0x2001FFF0", // RAM test at the top (SRAM2)
    "    ldr r2, =0x5AA5C33C",
    "    str r2, [r1]",
    "    ldr r3, [r1]",
    "    cmp r3, r2",
    "    bne 1f",
    "    ldr r1, =0x00600000", // both on
    "    str r1, [r0]",
    "    bx lr",
    "1:  ldr r1, =0x00600000", // read-back wrong: both blink fast forever
    "    str r1, [r0]",
    "    ldr r2, =400000",
    "2:  subs r2, r2, #1",
    "    bne 2b",
    "    movs r1, #0x60",
    "    str r1, [r0]",
    "    ldr r2, =400000",
    "3:  subs r2, r2, #1",
    "    bne 3b",
    "    b 1b",
    ".ltorg",
);
