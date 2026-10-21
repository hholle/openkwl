//! SNTP client: packet logic and retry policy from `openkwl_core::sntp` (host-tested); this
//! task does DNS and UDP and hands the time to the control loop, which sets the RTC.
//!
//! Server choice: the configured one; otherwise the router (DHCP gateway), which on most home
//! routers (FRITZ!Box, OpenWrt, …) answers NTP; after a failure there, `pool.ntp.org`.
//! embassy-net does not pass DHCP option 42 through (its DHCP socket consumes the replies),
//! so the gateway stands in for the announced server.

use core::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};
use defmt::{info, warn};
use embassy_net::{
    Stack,
    dns::DnsQueryType,
    udp::{PacketMetadata, UdpSocket},
};

use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex},
    signal::Signal,
};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use openkwl_core::sntp::{self, SyncPolicy};

use crate::{SharedApp, SharedRng};

/// A successful sync: UTC seconds, for the control loop to put into the RTC.
pub static SYNCED: Signal<CriticalSectionRawMutex, u64> = Signal::new();
/// Server of the last attempt and successful syncs since boot (`GET /system/details`).
pub static SERVER: BlockingMutex<CriticalSectionRawMutex, RefCell<heapless::String<64>>> =
    BlockingMutex::new(RefCell::new(heapless::String::new()));
pub static SYNCS: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
pub async fn sntp_task(stack: Stack<'static>, app: &'static SharedApp, rng: &'static SharedRng) -> ! {
    let mut policy = SyncPolicy::new();
    let mut try_router = true;
    let (mut rx_meta, mut tx_meta) = ([PacketMetadata::EMPTY; 2], [PacketMetadata::EMPTY; 2]);
    let (mut rx, mut tx) = ([0u8; 128], [0u8; 128]);
    loop {
        stack.wait_config_up().await;
        let now = Instant::now().as_secs();
        if !policy.due(now) {
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }
        let mut router: heapless::String<15> = heapless::String::new();
        if let Some(gw) = stack.config_v4().and_then(|c| c.gateway)
            && try_router
        {
            let _ = core::fmt::write(&mut router, format_args!("{gw}"));
        }
        let server: heapless::String<64> = {
            let a = app.lock().await;
            let dhcp = a.config().network.dhcp && !router.is_empty();
            match a.config().time.server(dhcp.then_some(router.as_str())) {
                Some(s) => heapless::String::try_from(s).unwrap_or_default(),
                None => {
                    drop(a);
                    Timer::after(Duration::from_secs(60)).await;
                    continue;
                }
            }
        };
        SERVER.lock(|s| *s.borrow_mut() = server.clone());
        let result = async {
            let addr = *stack.dns_query(&server, DnsQueryType::A).await.map_err(|_| None)?.first().ok_or(None)?;
            let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
            socket.bind(0).map_err(|_| None)?;
            let mut nonce = [0u8; 8];
            rng.lock().await.fill_bytes(&mut nonce);
            let request = sntp::request(nonce);
            socket.send_to(&request, (addr, sntp::PORT)).await.map_err(|_| None)?;
            let mut reply = [0u8; 64];
            let (n, _) = with_timeout(Duration::from_secs(3), socket.recv_from(&mut reply))
                .await
                .map_err(|_| None)?
                .map_err(|_| None)?;
            sntp::parse(&reply[..n], &request).map_err(Some)
        }
        .await;
        match result {
            Ok(r) => {
                info!("sntp: {} (stratum {})", r.unix_ms / 1000, r.stratum);
                SYNCED.signal(r.unix_ms / 1000);
                SYNCS.fetch_add(1, Ordering::Relaxed);
                policy.on_success(now);
            }
            Err(e) => {
                warn!("sntp: {} failed", server.as_str());
                // Router without NTP: use the pool from now on (until the next restart).
                if server.as_str() == router.as_str() {
                    try_router = false;
                }
                policy.on_failure(now, e);
            }
        }
    }
}
