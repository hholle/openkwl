//! Ethernet (RMII, Micrel KSZ PHY, see `phy.rs`) and the IP stack: DHCP or the static address from
//! `CONFIG.INI`, host name for DHCP, current address for `GET /system`.

use core::{cell::RefCell, str::FromStr, sync::atomic::Ordering};

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Ipv4Address, Ipv4Cidr, Stack, StackResources, StaticConfigV4};
use embassy_stm32::{
    Peri,
    eth::{Ethernet, PacketQueue, Sma},
    gpio::{Level, Output, Speed},
    peripherals::{ETH, ETH_SMA, PA1, PA2, PA7, PB11, PB12, PB13, PC1, PC4, PC5, PD15},
};
use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex},
    mutex::Mutex as AsyncMutex,
};
use openkwl_api::NetworkConfig;
use rand_core::RngCore;
use static_cell::{ConstStaticCell, StaticCell};

use crate::Irqs;

pub type Device = Ethernet<'static, ETH, crate::phy::KszPhy<Sma<'static, ETH_SMA>>>;

/// Current IPv4 address (DHCP lease or static), for `GET /system`.
pub static ADDRESS: AsyncMutex<CriticalSectionRawMutex, Option<Ipv4Address>> = AsyncMutex::new(None);

/// RMII pins and the PHY reset (as the stock firmware configures them, see `board.rs`).
pub struct Pins {
    pub eth: Peri<'static, ETH>,
    pub ref_clk: Peri<'static, PA1>,
    pub crs: Peri<'static, PA7>,
    pub rx_d0: Peri<'static, PC4>,
    pub rx_d1: Peri<'static, PC5>,
    pub tx_d0: Peri<'static, PB12>,
    pub tx_d1: Peri<'static, PB13>,
    pub tx_en: Peri<'static, PB11>,
    pub sma: Peri<'static, ETH_SMA>,
    pub mdio: Peri<'static, PA2>,
    pub mdc: Peri<'static, PC1>,
    pub phy_reset: Peri<'static, PD15>,
}

fn ip_config(n: &NetworkConfig) -> embassy_net::Config {
    let parse = |s: &Option<openkwl_api::config::Ipv4String>| s.as_ref().and_then(|s| Ipv4Address::from_str(s).ok());
    if !n.dhcp
        && let (Some(addr), Some(mask)) = (parse(&n.address), parse(&n.netmask))
    {
        let prefix = u32::from(mask).count_ones() as u8;
        let mut cfg =
            StaticConfigV4 { address: Ipv4Cidr::new(addr, prefix), gateway: parse(&n.gateway), dns_servers: Default::default() };
        if let Some(d) = parse(&n.dns) {
            let _ = cfg.dns_servers.push(d);
        }
        return embassy_net::Config::ipv4_static(cfg);
    }
    let mut dhcp = embassy_net::DhcpConfig::default();
    dhcp.hostname = n.hostname.as_str().try_into().ok();
    embassy_net::Config::dhcpv4(dhcp)
}

/// Whether the MAC gets the 50 MHz RMII reference clock (PA1, from the PHY or its oscillator):
/// the DMA soft reset completes only with it. embassy's driver waits for that without a limit,
/// which would stop the normal executor and with it, through the watchdog, the whole unit.
#[inline(never)]
fn ref_clock_runs() -> bool {
    use embassy_stm32::pac::{self, gpio::vals::Moder};
    // RMII selected before the MAC clocks start, PA1 as ETH_REF_CLK (AF11), then the reset;
    // `Ethernet::new_with_phy` does the same again, and its reset then completes at once.
    pac::SYSCFG.pmc().modify(|w| w.set_mii_rmii_sel(true));
    pac::GPIOA.afr(0).modify(|w| w.set_afr(1, 11));
    pac::GPIOA.moder().modify(|w| w.set_moder(1, Moder::ALTERNATE));
    pac::RCC.ahb1enr().modify(|w| {
        w.set_ethen(true);
        w.set_ethtxen(true);
        w.set_ethrxen(true);
    });
    let dmabmr = pac::ETH.ethernet_dma().dmabmr();
    dmabmr.modify(|w| w.set_sr(true));
    // With the clock it takes a few microseconds; this allows about 10 ms.
    (0..200_000).any(|_| !dmabmr.read().sr())
}

/// Starts Ethernet and the IP stack; `None` without the RMII reference clock (dead PHY or
/// oscillator): the unit then runs without a network, but keeps ventilating and saving.
pub async fn start(
    spawner: &Spawner,
    pins: Pins,
    mac: [u8; 6],
    network: &NetworkConfig,
    rng: &mut impl RngCore,
) -> Option<Stack<'static>> {
    // PHY reset like the stock firmware: low 2 ms, then 100 ms to come up. The pin stays high.
    let mut reset = Output::new(pins.phy_reset, Level::Low, Speed::Low);
    embassy_time::Timer::after_millis(2).await;
    reset.set_high();
    core::mem::forget(reset);
    embassy_time::Timer::after_millis(100).await;
    if !ref_clock_runs() {
        defmt::warn!("eth: no 50 MHz reference clock, running without a network");
        crate::phy::set_model(crate::phy::Model::NoClock);
        NETWORK.lock(|s| s.borrow_mut().phy = crate::phy::model_name().try_into().unwrap_or_default());
        return None;
    }

    // Built at compile time: `StaticCell::init` would assemble the 12 KB queue on the stack first.
    static PACKETS: ConstStaticCell<PacketQueue<4, 4>> = ConstStaticCell::new(PacketQueue::new());
    let phy = crate::phy::KszPhy::new(Sma::new(pins.sma, pins.mdio, pins.mdc));
    let device = Ethernet::new_with_phy(
        PACKETS.take(),
        pins.eth,
        Irqs,
        pins.ref_clk,
        pins.crs,
        pins.rx_d0,
        pins.rx_d1,
        pins.tx_d0,
        pins.tx_d1,
        pins.tx_en,
        mac,
        phy,
    );
    // HTTP workers + MQTT + SNTP + mDNS + DHCP + DNS = 8, plus room: a full socket set panics.
    static RESOURCES: StaticCell<StackResources<{ crate::http::WORKERS + 7 }>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(device, ip_config(network), RESOURCES.init(StackResources::new()), rng.next_u64());
    spawner.spawn(crate::task(net_task(runner)));
    spawner.spawn(crate::task(net_status_task(stack)));
    Some(stack)
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device>) -> ! {
    runner.run().await
}

fn ip_string(ip: embassy_net::Ipv4Address) -> openkwl_api::config::Ipv4String {
    let mut s = heapless::String::new();
    let _ = core::fmt::write(&mut s, format_args!("{ip}"));
    s
}

/// Link and IP state for `GET /system` and `GET /system/details` (`dhcp` is set by the
/// control loop from the settings).
fn network(stack: &Stack<'static>) -> openkwl_api::NetworkDetails {
    let cfg = stack.config_v4();
    openkwl_api::NetworkDetails {
        link_up: stack.is_link_up(),
        ip: cfg.as_ref().map(|c| ip_string(c.address.address())),
        prefix_len: cfg.as_ref().map(|c| c.address.prefix_len()),
        gateway: cfg.as_ref().and_then(|c| c.gateway).map(ip_string),
        dns: cfg
            .as_ref()
            .map(|c| c.dns_servers.iter().take(3).map(|d| ip_string(*d)).collect())
            .unwrap_or_default(),
        // Not available from embassy-net (see sntp.rs).
        ntp_server: None,
        mdns: crate::mdns::ACTIVE.load(Ordering::Relaxed),
        phy: crate::phy::model_name().try_into().unwrap_or_default(),
        ..Default::default()
    }
}

/// Snapshot of [`network`] for the control loop, which runs at a higher priority and must not
/// touch the IP stack itself.
pub static NETWORK: BlockingMutex<CriticalSectionRawMutex, RefCell<openkwl_api::NetworkDetails>> =
    BlockingMutex::new(RefCell::new(openkwl_api::NetworkDetails {
        link_up: false,
        dhcp: false,
        hostname: heapless::String::new(),
        mac: heapless::String::new(),
        ip: None,
        prefix_len: None,
        gateway: None,
        dns: heapless::Vec::new(),
        ntp_server: None,
        mdns: false,
        phy: heapless::String::new(),
    }));

/// Also tracks the current address for `GET /system` (a DHCP renewal can change it without
/// the configuration going down).
#[embassy_executor::task]
pub async fn net_status_task(stack: Stack<'static>) -> ! {
    loop {
        let addr = stack.config_v4().map(|c| c.address.address());
        {
            let mut current = ADDRESS.lock().await;
            if *current != addr {
                if let Some(a) = addr {
                    info!("net: address {}", a);
                }
                *current = addr;
            }
        }
        let n = network(&stack);
        NETWORK.lock(|s| *s.borrow_mut() = n);
        embassy_time::Timer::after_secs(1).await;
    }
}
