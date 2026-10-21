//! mDNS responder: answers `<hostname>.local` with the current address, so the unit is
//! reachable as e.g. `http://openkwl.local/` without knowing its IP. Packet logic in
//! `openkwl_core::mdns` (host-tested); this task owns the socket and the multicast group.

use core::sync::atomic::{AtomicBool, Ordering};

use defmt::{debug, warn};
use embassy_futures::select::{Either3, select3};
use embassy_net::{
    IpAddress, IpEndpoint, Ipv4Address, Stack,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_time::Timer;
use openkwl_core::mdns::{self, Reply};

use crate::SharedApp;

/// The responder is joined to the group and answering (`GET /system/details`).
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

const GROUP: Ipv4Address = Ipv4Address::new(mdns::GROUP[0], mdns::GROUP[1], mdns::GROUP[2], mdns::GROUP[3]);

#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>, app: &'static SharedApp) -> ! {
    let (mut rx_meta, mut tx_meta) = ([PacketMetadata::EMPTY; 2], [PacketMetadata::EMPTY; 2]);
    let (mut rx, mut tx) = ([0u8; 512], [0u8; 256]);
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    if socket.bind(mdns::PORT).is_err() {
        warn!("mdns: port 5353 busy");
    }
    let group = IpEndpoint::new(IpAddress::Ipv4(GROUP), mdns::PORT);
    let mut packet = [0u8; 512];
    let mut reply = [0u8; 300];
    loop {
        ACTIVE.store(false, Ordering::Relaxed);
        stack.wait_config_up().await;
        let Some(ip) = stack.config_v4().map(|c| c.address.address().octets()) else { continue };
        let mut host: heapless::String<32> = heapless::String::new();
        for c in app.lock().await.config().network.hostname.chars() {
            let _ = host.push(c.to_ascii_lowercase());
        }
        if stack.join_multicast_group(IpAddress::Ipv4(GROUP)).is_err() {
            warn!("mdns: cannot join 224.0.0.251");
            Timer::after_secs(60).await;
            continue;
        }
        ACTIVE.store(true, Ordering::Relaxed);
        debug!("mdns: answering {}.local", host.as_str());
        // Announce twice, one second apart (RFC 6762 §8.3).
        for _ in 0..2 {
            if let Some(n) = mdns::announcement(&host, ip, &mut reply) {
                let _ = socket.send_to(&reply[..n], group).await;
            }
            Timer::after_secs(1).await;
        }
        loop {
            match select3(socket.recv_from(&mut packet), stack.wait_config_down(), Timer::after_secs(5)).await {
                Either3::First(Ok((n, meta))) => match mdns::answer(&packet[..n], meta.endpoint.port, &host, ip, &mut reply) {
                    Some(Reply::Multicast(len)) => {
                        let _ = socket.send_to(&reply[..len], group).await;
                    }
                    Some(Reply::Unicast(len)) => {
                        let _ = socket.send_to(&reply[..len], meta.endpoint).await;
                    }
                    None => {}
                },
                Either3::First(Err(_)) => {}
                // Address gone (cable out, lease lost): announce the new one when it is back.
                Either3::Second(()) => break,
                // A DHCP renewal can change the address without the configuration going down:
                // answer with and announce the new one. (A new host name takes effect after a
                // restart, like the other network settings.)
                Either3::Third(()) => {
                    if stack.config_v4().map(|c| c.address.address().octets()) != Some(ip) {
                        break;
                    }
                }
            }
        }
    }
}
