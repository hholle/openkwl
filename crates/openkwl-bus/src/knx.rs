//! The KWL-KNX module (Modbus slave 0x14), exactly as the stock controller serves it
//! (observed protocol behaviour; the frames in `knx/tests.rs` are the reference).
//!
//! Every second the controller reads the module's 16 input registers (function 04). In them
//! the module may ask for a "block" (a copy of one of the stock firmware's RAM structs), which
//! the controller then writes (function 16), or offer one, which the controller reads
//! (function 03). Register 12 carries the fan level and the automatic flag from KNX.
//!
//! This module has no I/O: the firmware and the simulator move the frames.

use crate::modbus::{self, Error, FRAME_MAX};

/// The module's fixed address ("Adresse intern fest").
pub const ADDRESS: u8 = 0x14;
const INPUT_REGS: usize = 16;
/// "Invalid" for a temperature or sensor value.
const INVALID_F32: f32 = 9999.0;
const INVALID_U16: u16 = 9999;
/// Weekly ventilation programme "off" in block 15.
pub const PROGRAMME_OFF: u8 = 5;

/// Local date and time as the module expects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    /// 1 = Monday … 7 = Sunday.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// What the controller reports to the module. The words use the stock bit layout
/// (`openkwl-app` fills them).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Snapshot {
    pub level: u8,
    /// Weekly programme in charge (not manual).
    pub auto: bool,
    /// Fan output, 0–100 %.
    pub fan_pct: u8,
    /// `None` while the clock is not set (all fields 0).
    pub time: Option<DateTime>,
    pub status: u32,
    pub errors: u32,
    pub warnings: u32,
    pub info: u32,
    /// Block 1 +0x25: bit 4 link up; low nibble 1 static, 2 DHCP running, 4 DHCP bound.
    pub network: u8,
    pub uptime_ms: u32,
    /// Outdoor, supply, extract, exhaust air.
    pub temperatures: [Option<f32>; 4],
    /// Active weekly programme 0–4 (stock Standard 1–3, Individuell 1–2), [`PROGRAMME_OFF`].
    pub programme: u8,
    pub filter_months: u8,
    pub filter_remaining_min: u32,
    pub supply_minutes: u32,
    pub extract_minutes: u32,
}

/// Struct length in bytes of a block the controller serves or accepts; 0 = unknown (the stock
/// answer to an unknown ID is an empty block).
pub fn block_len(id: u16) -> usize {
    match id {
        1 => 0x2C,
        15 | 56 => 4,
        23 => 0x10,
        31 | 32 => 0x24,
        33..=35 => 0x12,
        50 => 8,
        53 => 0x1C,
        _ => 0,
    }
}

/// The struct bytes of block `id`, little-endian like the stock RAM; returns the length.
/// Bytes the stock firmware never sets are 0.
fn struct_bytes(id: u16, s: &Snapshot, b: &mut [u8; 64]) -> usize {
    let len = block_len(id);
    b[..len].fill(0);
    let u16_at = |b: &mut [u8; 64], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let u32_at = |b: &mut [u8; 64], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let f32_at = |b: &mut [u8; 64], at: usize, v: f32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    match id {
        1 => {
            b[0] = (s.level & 0x0F) | if s.auto { 0x80 } else { 0 };
            b[1] = s.fan_pct;
            if let Some(t) = s.time {
                b[2] = t.month;
                b[3] = t.day;
                u16_at(b, 4, t.year % 100);
                b[6] = t.weekday;
                b[0x0A] = t.second;
                b[0x0B] = t.minute;
                b[0x0C] = t.hour;
            }
            // +7 date format 0 = DD.MM.YYYY; +8 a panel setting, default 0.
            u32_at(b, 0x10, s.status);
            u32_at(b, 0x14, s.errors);
            u32_at(b, 0x1C, s.warnings);
            u32_at(b, 0x20, s.info);
            // Fan levels for "supply air only" / "extract air only" (stock defaults 2 and 2).
            b[0x24] = 0x22;
            b[0x25] = s.network;
            u32_at(b, 0x28, s.uptime_ms);
        }
        15 => {
            u16_at(b, 0, 15);
            b[2] = s.programme;
        }
        23 => {
            // Heater programme: no heaters, so "off", with the stock set-points.
            u16_at(b, 0, 23);
            u16_at(b, 2, u16::from(PROGRAMME_OFF));
            f32_at(b, 4, 20.0);
            f32_at(b, 8, 17.0);
            f32_at(b, 0x0C, 35.0);
        }
        31 => {
            u16_at(b, 0, 31);
            for (i, t) in s.temperatures.iter().enumerate() {
                f32_at(b, 4 + 4 * i, t.unwrap_or(INVALID_F32));
            }
            // Duct sensors of a KWL-EM (none fitted).
            for at in [0x14, 0x18, 0x1C, 0x20] {
                f32_at(b, at, INVALID_F32);
            }
        }
        32 => {
            u16_at(b, 0, 32);
            for i in 0..8 {
                f32_at(b, 4 + 4 * i, INVALID_F32);
            }
        }
        33..=35 => {
            u16_at(b, 0, id);
            for i in 0..8 {
                u16_at(b, 2 + 2 * i, INVALID_U16);
            }
        }
        50 => {
            u16_at(b, 0, 50);
            b[3] = s.filter_months;
            u32_at(b, 4, s.filter_remaining_min);
        }
        53 => {
            u16_at(b, 0, 53);
            u32_at(b, 4, s.supply_minutes);
            u32_at(b, 8, s.extract_minutes);
            // Heater run times stay 0.
        }
        56 => {
            // Pre-heater status: none fitted, so disabled.
            u16_at(b, 0, 56);
        }
        _ => {}
    }
    len
}

/// The registers of a block write: {1, id, length} and the struct, two bytes per register
/// (`hi` = the odd byte, so every pair is swapped on the wire). Returns how many.
pub fn block_registers(id: u16, s: &Snapshot, regs: &mut [u16; 3 + 32]) -> usize {
    let mut b = [0u8; 64];
    let len = struct_bytes(id, s, &mut b);
    regs[0] = 1;
    regs[1] = id;
    regs[2] = len as u16;
    let words = len.div_ceil(2);
    for k in 0..words {
        regs[3 + k] = u16::from_le_bytes([b[2 * k], b[2 * k + 1]]);
    }
    3 + words
}

/// What a module may change by offering a block (function 03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// Block 15: active weekly ventilation programme 0–4, or [`PROGRAMME_OFF`].
    Programme(u8),
}

/// Reads an offered block; `None` for blocks that change nothing here (heater settings on a
/// unit without heaters, and the measured-value blocks the stock firmware wrongly accepts).
pub fn parse_push(regs: &[u16]) -> Option<Push> {
    let id = *regs.get(1)?;
    let byte = |i: usize| regs.get(3 + i / 2).map(|r| r.to_le_bytes()[i % 2]);
    match id {
        15 => Some(Push::Programme(byte(2)?)),
        _ => None,
    }
}

/// A fan command from the module (register 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanCommand {
    Auto,
    /// Manual at this level (0–4).
    Manual(u8),
}

/// Register 12 as the stock firmware applies it: changes only, and nothing on the first
/// answer after start-up (the module may still report an old state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanInput {
    skip: bool,
    last_auto: u8,
    last_level: u8,
}

impl Default for FanInput {
    fn default() -> Self {
        Self::new()
    }
}

impl FanInput {
    pub const fn new() -> Self {
        Self { skip: true, last_auto: 0, last_level: 0 }
    }

    /// `reg4` = the module's own address, `reg12` = its command; `auto` and `level` = the
    /// controller's state now.
    pub fn on_poll(&mut self, reg4: u16, reg12: u16, auto: bool, level: u8) -> Option<FanCommand> {
        if reg4 & 0xFF != u16::from(ADDRESS) {
            return None;
        }
        let cmd = reg12 as u8;
        let (hi, lo) = (cmd & 0xF0, cmd & 0x0F);
        if core::mem::take(&mut self.skip) {
            return None;
        }
        let mut command = None;
        let mut auto = auto;
        if self.last_auto != hi {
            self.last_auto = hi;
            // Stock stores any high nibble as the mode; only these two mean something.
            match hi {
                0x80 => {
                    auto = true;
                    command = Some(FanCommand::Auto);
                }
                0x00 => {
                    auto = false;
                    command = Some(FanCommand::Manual(level));
                }
                _ => {}
            }
        }
        if auto {
            self.last_level = level;
        } else if self.last_level != lo {
            self.last_level = lo;
            command = Some(FanCommand::Manual(lo.min(4)));
        }
        command
    }

    /// Serving block 1 shows the module the current level (stock updates its memory too).
    pub fn block1_served(&mut self, level: u8) {
        self.last_level = level;
    }
}

/// Whether the module answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Not answered yet since start-up.
    Searching,
    Connected,
    /// Stopped answering, or never did; still asked now and then.
    NotFound,
}

/// What to send after the module's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followup {
    /// Nothing more this round.
    None,
    /// A block write is in `out` (`len` bytes); the module echoes `qty`.
    Write { len: usize, qty: u16 },
    /// A read of an offered block is in `out`.
    Read { len: usize, qty: u16 },
}

/// Timeouts in a row before the module counts as gone (stock: 11, then never again until a
/// restart; here it is asked every [`ABSENT_POLL_MS`] instead).
const TIMEOUTS_GONE: u8 = 11;
/// Between polls while the module answers.
pub const POLL_MS: u32 = 1000;
/// Between attempts while it does not.
pub const ABSENT_POLL_MS: u32 = 10_000;
/// The whole answer must be there this long after the request was sent (stock 43.6 ms).
pub const ANSWER_TIMEOUT_MS: u32 = 50;

/// The KWL-KNX as the controller sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub presence: Presence,
    timeouts: u8,
    pub fan: FanInput,
    /// Register 5 low byte: 3 = application running.
    pub appl_status: u8,
    /// Firmware and bootloader version × 100.
    pub firmware: u32,
    pub bootloader: u32,
    pub good: u32,
    pub bad: u32,
    pub missed: u32,
}

impl Default for Module {
    fn default() -> Self {
        Self::new()
    }
}

impl Module {
    pub const fn new() -> Self {
        Self {
            presence: Presence::Searching,
            timeouts: 0,
            fan: FanInput::new(),
            appl_status: 0,
            firmware: 0,
            bootloader: 0,
            good: 0,
            bad: 0,
            missed: 0,
        }
    }

    /// The poll (function 04, all 16 input registers).
    pub fn request(out: &mut [u8; FRAME_MAX]) -> usize {
        modbus::read(out, ADDRESS, modbus::READ_INPUT, 0, INPUT_REGS as u16).unwrap_or(0)
    }

    /// Handles the answer to [`Module::request`]: returns a fan command (if register 12
    /// changed) and what to send next, which is written to `out`.
    pub fn on_answer(
        &mut self,
        answer: &[u8],
        s: &Snapshot,
        out: &mut [u8; FRAME_MAX],
    ) -> Result<(Option<FanCommand>, Followup), Error> {
        let mut r = [0u16; INPUT_REGS];
        if let Err(e) = modbus::read_answer(answer, ADDRESS, modbus::READ_INPUT, &mut r) {
            self.bad = self.bad.wrapping_add(1);
            return Err(e);
        }
        self.good = self.good.wrapping_add(1);
        self.timeouts = 0;
        self.presence = Presence::Connected;
        self.appl_status = r[5] as u8;
        self.bootloader = u32::from(r[8]) | u32::from(r[9]) << 16;
        self.firmware = u32::from(r[10]) | u32::from(r[11]) << 16;
        let command = self.fan.on_poll(r[4], r[12], s.auto, s.level);
        let id = r[1];
        let followup = match r[0] {
            1 => {
                let mut regs = [0u16; 35];
                let qty = block_registers(id, s, &mut regs);
                if id == 1 {
                    self.fan.block1_served(s.level);
                }
                let len = modbus::write_multiple(out, ADDRESS, 0, &regs[..qty])?;
                Followup::Write { len, qty: qty as u16 }
            }
            2 => {
                let qty = (block_len(id).div_ceil(2) + 3) as u16;
                let len = modbus::read(out, ADDRESS, modbus::READ_HOLDING, 0, qty)?;
                Followup::Read { len, qty }
            }
            _ => Followup::None,
        };
        Ok((command, followup))
    }

    /// Handles the answer to a [`Followup`]; returns what an offered block changes.
    pub fn on_followup_answer(&mut self, answer: &[u8], f: Followup) -> Result<Option<Push>, Error> {
        let result = match f {
            Followup::None => return Ok(None),
            Followup::Write { qty, .. } => modbus::write_answer(answer, ADDRESS, 0, qty).map(|()| None),
            Followup::Read { qty, .. } => {
                let mut regs = [0u16; 3 + 32];
                let regs = regs.get_mut(..usize::from(qty)).ok_or(Error::Request)?;
                modbus::read_answer(answer, ADDRESS, modbus::READ_HOLDING, regs).map(|()| parse_push(regs))
            }
        };
        match result {
            Ok(_) => self.good = self.good.wrapping_add(1),
            Err(_) => self.bad = self.bad.wrapping_add(1),
        }
        result
    }

    /// No (complete) answer in time.
    pub fn on_timeout(&mut self) {
        self.missed = self.missed.wrapping_add(1);
        self.timeouts = self.timeouts.saturating_add(1);
        if self.timeouts >= TIMEOUTS_GONE {
            self.presence = Presence::NotFound;
        }
    }

    /// Milliseconds until the next poll (stock back-off after timeouts).
    pub fn next_poll_ms(&self) -> u32 {
        match (self.presence, self.timeouts) {
            (Presence::NotFound, _) => ABSENT_POLL_MS,
            (_, 0 | 1) => POLL_MS,
            (_, 2) => 1400,
            (_, 3) => 2400,
            (_, 4 | 5) => 3400,
            _ => 4400,
        }
    }
}

#[cfg(test)]
mod tests;
