//! `PUT /api/v1/system/update`: receives an update package (`openkwl-update`) straight onto the
//! SD card while it is checked, and arms the bootloader only when everything checked out. The
//! unit installs it at the next restart, which the web UI asks for.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_net::tcp::TcpSocket;
use openkwl_update::Reader;

use crate::{SharedApp, storage::Card};

/// Largest package accepted: the image plus a web UI with room to grow.
const PACKAGE_MAX: usize = 2 * 1024 * 1024;

/// One upload at a time.
static BUSY: AtomicBool = AtomicBool::new(false);

/// What went wrong, as HTTP status, API error code and message.
pub type Failure = (u16, &'static str, &'static str);
pub const NO_CARD: Failure = (503, "unavailable", "the SD card could not store the update");
const ARMED: Failure = (409, "conflict", "an update is ready: restart the unit to install it first");

/// Receives `length` bytes, of which `have` arrived with the header at the start of `buf` (the
/// worker's request buffer, reused for the rest). The work happens in plain functions on
/// `Storage`; this loop only waits for the network and the card.
pub async fn receive(
    socket: &mut TcpSocket<'_>,
    length: usize,
    mut have: usize,
    buf: &mut [u8],
    app: &'static SharedApp,
    card: &'static Card,
) -> Result<(), Failure> {
    if length > PACKAGE_MAX {
        return Err((413, "payload_too_large", "the update file is too large"));
    }
    if BUSY.swap(true, Ordering::Acquire) {
        return Err((409, "conflict", "another update is being uploaded"));
    }
    let mut reader = Reader::new();
    let mut received = 0;
    // An armed update (not yet installed) would lose its web folder to this one: refuse, but
    // still take the body below, so the browser gets the answer.
    let armed = card.lock().await.as_deref().is_some_and(|s| s.update_armed());
    let mut result = if armed { Err(ARMED) } else { Ok(()) };
    loop {
        if have > 0 {
            if result.is_ok() {
                result = card.lock().await.as_deref_mut().map_or(Err(NO_CARD), |s| s.update_piece(&mut reader, &buf[..have]));
            }
            received += have;
        }
        if result.is_err() || received >= length {
            break;
        }
        let want = buf.len().min(length - received);
        have = match socket.read(&mut buf[..want]).await {
            Ok(n) if n > 0 => n,
            _ => {
                result = Err((400, "bad_request", "the upload was cut off"));
                break;
            }
        };
    }
    // After a content error the browser is still sending: take the rest, or the connection is
    // reset before it sees the answer.
    while result.is_err() && received < length {
        let want = buf.len().min(length - received);
        match socket.read(&mut buf[..want]).await {
            Ok(n) if n > 0 => received += n,
            _ => break,
        }
    }
    let result = card.lock().await.as_deref_mut().map_or(Err(NO_CARD), |s| s.update_end(result, &reader));
    BUSY.store(false, Ordering::Release);
    let (crc, files) = result?;
    app.lock().await.update_ready(crc, files);
    Ok(())
}
