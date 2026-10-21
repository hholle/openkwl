//! Debug build without a probe (`--no-default-features --features sd-log`): every `defmt`
//! message goes into a RAM ring instead of RTT, and the storage task appends the ring to
//! `HELIOS/DEFMT.LOG` on the card every few seconds.
//!
//! - The ring lives in `.uninit`, so whatever was not written yet survives a watchdog reset,
//!   a panic or a hard fault (both are logged, then the unit restarts) and reaches the card
//!   after the restart.
//! - The file holds raw defmt frames, like RTT. Decode it on a computer with the ELF of the
//!   exact same build: `defmt-print -e target/thumbv7m-none-eabi/release/openkwl-fw < DEFMT.LOG`.
//! - When the ring is full, new messages are dropped (and counted) until the card catches up.

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

/// Room for a few seconds of messages: the storage task empties it every [`FLUSH_S`]. It
/// comes out of the stack's share of RAM, so keep it small.
const RING_LEN: usize = 2048;
pub const FLUSH_S: u64 = 5;
const MAGIC: u32 = 0x5344_4C47; // "SDLG"
/// The file restarts empty beyond this size (debug builds only, so the write budget is moot).
pub const FILE_MAX: u32 = 4 * 1024 * 1024;

struct Ring {
    magic: u32,
    /// Bytes written / taken since the ring was set up; `head - tail` are waiting.
    head: u32,
    tail: u32,
    dropped: u32,
    buf: [u8; RING_LEN],
}

struct Shared(UnsafeCell<MaybeUninit<Ring>>);
// SAFETY: only accessed inside critical sections.
unsafe impl Sync for Shared {}

#[unsafe(link_section = ".uninit.SDLOG")]
static RING: Shared = Shared(UnsafeCell::new(MaybeUninit::uninit()));

/// The ring, set up on first use unless a previous run left a valid one. Call inside a
/// critical section.
fn ring() -> &'static mut Ring {
    // SAFETY: callers hold a critical section; every field is plain data, so whatever the RAM
    // held is a valid (if meaningless) `Ring` until the magic check below resets it.
    let r = unsafe { (*RING.0.get()).assume_init_mut() };
    let sane = r.magic == MAGIC && r.head.wrapping_sub(r.tail) as usize <= RING_LEN;
    if !sane {
        *r = Ring { magic: MAGIC, head: 0, tail: 0, dropped: 0, buf: [0; RING_LEN] };
    }
    r
}

fn push(bytes: &[u8]) {
    let r = ring();
    if r.head.wrapping_sub(r.tail) as usize + bytes.len() > RING_LEN {
        r.dropped = r.dropped.wrapping_add(1);
        return;
    }
    for &b in bytes {
        r.buf[r.head as usize % RING_LEN] = b;
        r.head = r.head.wrapping_add(1);
    }
}

/// Moves waiting bytes into `out`; returns how many, and the dropped-message count to report
/// (reset by this call).
pub fn take(out: &mut [u8]) -> (usize, u32) {
    critical_section::with(|_| {
        let r = ring();
        let n = (r.head.wrapping_sub(r.tail) as usize).min(out.len());
        for b in &mut out[..n] {
            *b = r.buf[r.tail as usize % RING_LEN];
            r.tail = r.tail.wrapping_add(1);
        }
        (n, core::mem::take(&mut r.dropped))
    })
}

#[defmt::global_logger]
struct Logger;

/// Nesting depth: a message logged while another is being written (a fault handler) is dropped.
static DEPTH: AtomicU8 = AtomicU8::new(0);
struct State {
    restore: UnsafeCell<critical_section::RestoreState>,
    encoder: UnsafeCell<defmt::Encoder>,
}
// SAFETY: only used between `acquire` and `release`, inside the critical section they hold.
unsafe impl Sync for State {}
static STATE: State =
    State { restore: UnsafeCell::new(critical_section::RestoreState::invalid()), encoder: UnsafeCell::new(defmt::Encoder::new()) };

// SAFETY: the same protocol as `defmt-rtt`: a critical section from `acquire` to `release`.
// A nested message (logged from a fault inside a log call) gets no frame and is dropped.
unsafe impl defmt::Logger for Logger {
    fn acquire() {
        // SAFETY: released in `release` (outermost level) or right here (nested).
        let restore = unsafe { critical_section::acquire() };
        if DEPTH.fetch_add(1, Ordering::Relaxed) > 0 {
            unsafe { critical_section::release(restore) };
            return;
        }
        unsafe {
            *STATE.restore.get() = restore;
            (*STATE.encoder.get()).start_frame(push);
        }
    }

    unsafe fn flush() {}

    unsafe fn release() {
        if DEPTH.fetch_sub(1, Ordering::Relaxed) > 1 {
            return;
        }
        unsafe {
            (*STATE.encoder.get()).end_frame(push);
            critical_section::release(*STATE.restore.get());
        }
    }

    unsafe fn write(bytes: &[u8]) {
        if DEPTH.load(Ordering::Relaxed) == 1 {
            unsafe { (*STATE.encoder.get()).write(bytes, push) };
        }
    }
}

/// Logs the panic, then restarts; the message reaches the card after the restart (the ring
/// survives it). Without the location: keeping every panic site's file and line costs about
/// 20 KB of flash, more than the slot has. The messages logged just before usually tell
/// where it was; for the exact place use a probe and `--features panic-messages`.
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic, restarting");
    cortex_m::peripheral::SCB::sys_reset();
}

#[cortex_m_rt::exception]
unsafe fn HardFault(frame: &cortex_m_rt::ExceptionFrame) -> ! {
    defmt::error!("hard fault: pc={=u32:#x} lr={=u32:#x}", frame.pc(), frame.lr());
    cortex_m::peripheral::SCB::sys_reset();
}
