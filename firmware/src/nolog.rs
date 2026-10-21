//! Release build without a debug probe (`rtt`) or SD log (`sd-log`): the defmt log goes
//! nowhere (build with `DEFMT_LOG=off` and the log calls disappear entirely), and a panic
//! restarts the unit at once instead of waiting for the watchdog.

#[defmt::global_logger]
struct Nowhere;

// SAFETY: nothing is written anywhere, so there is no state to protect.
unsafe impl defmt::Logger for Nowhere {
    fn acquire() {}
    unsafe fn flush() {}
    unsafe fn release() {}
    unsafe fn write(_: &[u8]) {}
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}
