//! Ethernet PHY as the stock firmware drives it:
//! Micrel/Microchip KSZ8031RNL/KSZ8051RNL (ID 0x0022/0x155x, 2.24) or KSZ8081RNA/RND
//! (0x0022/0x156x, added in 2.27) in RMII mode with an external 50 MHz reference clock. The
//! address is scanned like the stock firmware does. Unlike embassy's `GenericPhy`, a missing PHY
//! does not panic: the unit must keep ventilating without a network.

use core::{
    future::Future,
    pin::pin,
    sync::atomic::{AtomicU8, Ordering},
    task::Context,
};

use embassy_stm32::eth::{Phy, StationManagement};
use embassy_time::{Duration, Timer, block_for};

const BCR: u8 = 0x00;
const BSR: u8 = 0x01;
const ID1: u8 = 0x02;
const ID2: u8 = 0x03;
/// KSZ PHY control 2; bit 7 = RMII 50 MHz clock mode.
const KSZ_CTRL2: u8 = 0x1F;

const BCR_RESET: u16 = 1 << 15;
const BCR_100M: u16 = 1 << 13;
const BCR_AN: u16 = 1 << 12;
const BCR_ANRST: u16 = 1 << 9;
const BSR_UP: u16 = 1 << 2;
const BSR_ANDONE: u16 = 1 << 5;

/// Which PHY was found, for `GET /system/details`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Model {
    None = 0,
    Ksz8031 = 1,
    Ksz8081 = 2,
    Other = 3,
    /// The MAC gets no RMII reference clock (`net::start`): no network.
    NoClock = 4,
}

static MODEL: AtomicU8 = AtomicU8::new(Model::None as u8);

const NO_CLOCK: &str = "no 50 MHz clock";

/// `net::start` found no RMII reference clock and left the network off.
pub fn no_clock() -> bool {
    MODEL.load(Ordering::Relaxed) == Model::NoClock as u8
}

pub fn model_name() -> &'static str {
    match MODEL.load(Ordering::Relaxed) {
        1 => "KSZ8031/8051RNL",
        2 => "KSZ8081RNA/RND",
        3 => "unknown PHY",
        4 => NO_CLOCK,
        _ => "no PHY found",
    }
}

pub fn set_model(model: Model) {
    MODEL.store(model as u8, Ordering::Relaxed);
}

pub struct KszPhy<SM> {
    sm: SM,
    addr: Option<u8>,
}

impl<SM: StationManagement> KszPhy<SM> {
    pub fn new(sm: SM) -> Self {
        Self { sm, addr: None }
    }

    /// Stock order: addresses 1…31, then 0 (the stock loop's 32 masks to 0).
    fn scan(&mut self) -> Option<(u8, Model)> {
        (1..32u8).chain([0]).find_map(|a| {
            let (id1, id2) = (self.sm.smi_read(a, ID1), self.sm.smi_read(a, ID2));
            match (id1, id2 & 0xFFF0) {
                (0x0022, 0x1550) => Some((a, Model::Ksz8031)),
                (0x0022, 0x1560) => Some((a, Model::Ksz8081)),
                (0xFFFF, _) | (0, _) => None,
                _ => Some((a, Model::Other)),
            }
        })
    }
}

impl<SM: StationManagement> Phy for KszPhy<SM> {
    fn phy_reset(&mut self) {
        let Some((addr, model)) = self.scan() else {
            defmt::warn!("eth: no PHY answers on the management bus");
            return;
        };
        defmt::info!("eth: PHY at address {}", addr);
        crate::sysinfo::board_report(|b| b.phy_address = Some(addr));
        set_model(model);
        self.addr = Some(addr);
        self.sm.smi_write(addr, BCR, BCR_RESET);
        for _ in 0..100 {
            if self.sm.smi_read(addr, BCR) & BCR_RESET == 0 {
                break;
            }
            block_for(Duration::from_millis(1));
        }
        // 2.27 sets the 50 MHz clock mode only on the KSZ8031/8051; the KSZ8081RND defaults to it.
        if model == Model::Ksz8031 {
            let v = self.sm.smi_read(addr, KSZ_CTRL2);
            self.sm.smi_write(addr, KSZ_CTRL2, v | 0x80);
        }
    }

    fn phy_init(&mut self) {
        if let Some(addr) = self.addr {
            self.sm.smi_write(addr, BCR, BCR_AN | BCR_ANRST | BCR_100M);
        }
    }

    fn poll_link(&mut self, cx: &mut Context) -> bool {
        // Registers the waker: the driver polls again in 500 ms.
        let _ = pin!(Timer::after(Duration::from_millis(500))).poll(cx);
        let Some(addr) = self.addr else { return false };
        let bsr = self.sm.smi_read(addr, BSR);
        bsr & BSR_ANDONE != 0 && bsr & BSR_UP != 0
    }
}
