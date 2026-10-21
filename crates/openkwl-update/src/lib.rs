//! Update packages for the web UI's firmware update (`PUT /api/v1/system/update`).
//!
//! One file holds the firmware image and the web UI, so both always change together:
//!
//! ```text
//! "OKWLPKG1"
//! entry*:      name (12 bytes, 8.3, NUL-padded) | length (u32 LE) | data
//! terminator:  12 zero bytes | CRC-16/XMODEM of everything before this field (u32 LE)
//! ```
//!
//! The first entry is [`FIRMWARE`]: a complete image for the stock bootloader, checked by the
//! same rules as `openkwl-pack verify` (length, CRC trailer, stack pointer, vectors). The other
//! entries are web files. [`Reader`] checks everything while the upload streams through; the
//! platform arms the bootloader only after [`Event::Done`].
//!
//! Arming (the platform's job, in this order):
//! 1. the image is written to the card root under the one of [`IMAGE_FILES`] that
//!    `settings.ini` does not name ([`image_slot`]), so an armed, not yet installed update is
//!    never overwritten by a later upload that fails halfway;
//! 2. the web files go to the web folder that is *not* in use ([`WEB_DIRS`]), followed by
//!    [`MARKER`] holding the image's CRC: at boot, the firmware serves the folder whose marker
//!    matches its own CRC (in flash at `0x0803FE00`), so the web UI switches together with the
//!    firmware, and a failed install keeps the old pair;
//! 3. `settings.ini` gets a `FwBasis=` first line naming that file ([`switch_settings`]),
//!    after a one-time copy to `HELIOS/SETTINGS.BAK`. The bootloader installs at the next
//!    start and renames the file to `basback.bin`.

#![no_std]

#[cfg(test)]
extern crate std;

pub const MAGIC: &[u8; 8] = b"OKWLPKG1";
/// Name of the firmware entry, always first.
pub const FIRMWARE: &str = "FIRMWARE.BIN";
/// Image files in the card root, used alternately; `card.sh` writes the first.
pub const IMAGE_FILES: [&str; 2] = ["BAS90001.BIN", "BAS90002.BIN"];
/// First lines of `settings.ini` that make the bootloader install them; the spelling `card.sh`
/// writes, which the bootloader accepted (it reads 32 bytes and cuts at the line end).
pub const FW_BASIS_LINES: [&str; 2] = ["FwBasis=BAS90001.bin", "FwBasis=BAS90002.bin"];
/// Web folders, used alternately. `card.sh` fills the first one, without a marker.
pub const WEB_DIRS: [&str; 2] = ["WWW", "WWW2"];
/// File in a web folder naming the firmware it belongs to: the image CRC, 2 bytes LE.
pub const MARKER: &str = "IMAGE.CRC";

/// Image body (flash 0x08008000–0x0803FDFF), then the 4-byte CRC trailer.
pub const IMAGE_BODY: u32 = 0x37E00;
pub const IMAGE_LEN: u32 = IMAGE_BODY + 4;
const APP_BASE: u32 = 0x0800_8000;
const APP_END: u32 = APP_BASE + IMAGE_BODY;
/// Initial stack pointer the bootloader accepts: 0x2001xxxx, and 16 bytes below the end of
/// RAM because its jump pops from the new stack (`tools/openkwl-pack`).
const SP_MIN: u32 = 0x2001_0000;
const SP_MAX: u32 = 0x2001_FFFF - 16;
/// Initial SP, reset and the 14 system exception vectors.
const VECTORS: usize = 16;

const HEADER: usize = 16;
const NAME: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not an openKWL update package.
    Magic,
    /// The first entry is not a complete firmware image.
    NoFirmware,
    /// An entry name that is not a plain 8.3 name.
    Name,
    /// Stack pointer the bootloader would refuse.
    StackPointer,
    /// Reset or exception vector outside the image.
    Vector,
    /// The image's CRC trailer does not match its contents.
    ImageCrc,
    /// The package's own checksum does not match (damaged upload).
    PackageCrc,
    /// Bytes after the end of the package.
    Trailing,
    /// The platform could not store a file.
    Write,
}

impl Error {
    pub const fn message(self) -> &'static str {
        match self {
            Error::Magic => "not an openKWL update file",
            Error::NoFirmware => "the update file has no firmware image",
            Error::Name => "the update file has an invalid file name",
            Error::StackPointer | Error::Vector => "the firmware image would not start on this unit",
            Error::ImageCrc => "the firmware image is damaged (checksum)",
            Error::PackageCrc => "the update file is damaged (checksum)",
            Error::Trailing => "the update file has extra data at the end",
            Error::Write => "the SD card could not store the update",
        }
    }
}

/// One step of the CRC-16/XMODEM (poly 0x1021, init 0) the bootloader checks.
pub fn crc16(mut crc: u16, data: &[u8]) -> u16 {
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// What [`Reader::push`] hands to the platform.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// A new entry starts; `firmware` for the image (store in the root, [`image_slot`]).
    File { name: &'a str, len: u32, firmware: bool },
    /// The next bytes of the current entry.
    Data(&'a [u8]),
    /// Everything arrived and checked out: arm the update. `web_files` = 0 for a package
    /// with only a firmware image (e.g. the stock firmware): keep the web folders as they are.
    Done { image_crc: u16, web_files: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Magic,
    Header,
    Body,
    Done,
}

/// Checks a package while it streams through, in pieces of any size.
pub struct Reader {
    stage: Stage,
    head: [u8; HEADER],
    have: usize,
    /// Bytes left in the current entry.
    left: u32,
    crc: u16,
    entries: u16,
    /// Bytes of the image seen so far, its running CRC, the vector table and the trailer.
    image_at: u32,
    image_crc: u16,
    vectors: [u8; VECTORS * 4],
    trailer: [u8; 4],
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub const fn new() -> Self {
        Reader {
            stage: Stage::Magic,
            head: [0; HEADER],
            have: 0,
            left: 0,
            crc: 0,
            entries: 0,
            image_at: 0,
            image_crc: 0,
            vectors: [0; VECTORS * 4],
            trailer: [0; 4],
        }
    }

    /// Whether the whole package has been read and checked.
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// After the whole package: the image CRC and the number of web files.
    pub fn finished(&self) -> Option<(u16, u16)> {
        self.done().then(|| (self.image_crc, self.entries - 1))
    }

    /// Feeds the next piece; `sink` stores what it gets and returns `false` on failure.
    pub fn push(&mut self, mut input: &[u8], sink: &mut dyn FnMut(Event<'_>) -> bool) -> Result<(), Error> {
        while !input.is_empty() {
            match self.stage {
                Stage::Done => return Err(Error::Trailing),
                Stage::Magic | Stage::Header => {
                    let want = if self.stage == Stage::Magic { MAGIC.len() } else { HEADER };
                    let n = (want - self.have).min(input.len());
                    self.head[self.have..self.have + n].copy_from_slice(&input[..n]);
                    self.have += n;
                    input = &input[n..];
                    if self.have == want {
                        self.have = 0;
                        self.header(sink)?;
                    }
                }
                Stage::Body => {
                    let n = (self.left as usize).min(input.len());
                    let (piece, rest) = input.split_at(n);
                    input = rest;
                    self.crc = crc16(self.crc, piece);
                    if self.entries == 1 {
                        self.image(piece);
                    }
                    if !sink(Event::Data(piece)) {
                        return Err(Error::Write);
                    }
                    self.left -= n as u32;
                    if self.left == 0 {
                        if self.entries == 1 {
                            self.check_image()?;
                        }
                        self.stage = Stage::Header;
                    }
                }
            }
        }
        Ok(())
    }

    fn header(&mut self, sink: &mut dyn FnMut(Event<'_>) -> bool) -> Result<(), Error> {
        if self.stage == Stage::Magic {
            if &self.head[..MAGIC.len()] != MAGIC {
                return Err(Error::Magic);
            }
            self.crc = crc16(0, MAGIC);
            self.stage = Stage::Header;
            return Ok(());
        }
        let (name, len) = self.head.split_at(NAME);
        let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]);
        self.crc = crc16(self.crc, name);
        if name.iter().all(|&b| b == 0) {
            if self.entries == 0 {
                return Err(Error::NoFirmware);
            }
            if u32::from(self.crc) != len {
                return Err(Error::PackageCrc);
            }
            self.stage = Stage::Done;
            sink(Event::Done { image_crc: self.image_crc, web_files: self.entries - 1 });
            return Ok(());
        }
        self.crc = crc16(self.crc, &self.head[NAME..]);
        let end = name.iter().position(|&b| b == 0).unwrap_or(NAME);
        let name = core::str::from_utf8(&self.head[..end]).ok().filter(|n| valid_name(n)).ok_or(Error::Name)?;
        let firmware = self.entries == 0;
        if firmware != (name == FIRMWARE) || (firmware && len != IMAGE_LEN) {
            return Err(Error::NoFirmware);
        }
        if name == MARKER {
            return Err(Error::Name);
        }
        // A package can't hold 65 535 files (the web UI has about 15): a counter that wrapped
        // would treat a later entry as the firmware again.
        self.entries = self.entries.checked_add(1).ok_or(Error::Name)?;
        self.left = len;
        if !sink(Event::File { name, len, firmware }) {
            return Err(Error::Write);
        }
        self.stage = if len == 0 { Stage::Header } else { Stage::Body };
        Ok(())
    }

    fn image(&mut self, piece: &[u8]) {
        for &b in piece {
            let at = self.image_at as usize;
            if at < self.vectors.len() {
                self.vectors[at] = b;
            }
            if self.image_at < IMAGE_BODY {
                self.image_crc = crc16(self.image_crc, &[b]);
            } else {
                self.trailer[at - IMAGE_BODY as usize] = b;
            }
            self.image_at += 1;
        }
    }

    fn check_image(&self) -> Result<(), Error> {
        let word = |i: usize| {
            u32::from_le_bytes([self.vectors[i * 4], self.vectors[i * 4 + 1], self.vectors[i * 4 + 2], self.vectors[i * 4 + 3]])
        };
        let sp = word(0);
        if !(SP_MIN..=SP_MAX).contains(&sp) || sp % 4 != 0 {
            return Err(Error::StackPointer);
        }
        // As `openkwl-pack verify`: reserved and unused slots are zero, the reset vector is not.
        for i in 1..VECTORS {
            let v = word(i);
            if (v != 0 || i == 1) && (v & 1 == 0 || !(APP_BASE..APP_END).contains(&(v & !1))) {
                return Err(Error::Vector);
            }
        }
        if self.trailer != [self.image_crc as u8, (self.image_crc >> 8) as u8, 0, 0] {
            return Err(Error::ImageCrc);
        }
        Ok(())
    }
}

/// Header of an entry, for building packages (`openkwl-pack bundle`). `name` must be a valid
/// 8.3 name of at most 12 bytes.
pub fn entry_header(name: &str, len: u32) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[NAME..].copy_from_slice(&len.to_le_bytes());
    h
}

/// The terminator, given the CRC of everything before it plus its 12 zero bytes.
pub fn terminator(crc_before: u16) -> [u8; HEADER] {
    let crc = crc16(crc_before, &[0; NAME]);
    entry_header("", u32::from(crc))
}

/// `NAME.EXT`: 1–8 and 0–3 characters of `A–Z 0–9 _ -`.
fn valid_name(name: &str) -> bool {
    let ok =
        |s: &str, max| s.len() <= max && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    let (base, ext) = name.split_once('.').unwrap_or((name, ""));
    !base.is_empty() && ok(base, 8) && ok(ext, 3)
}

fn first_line(settings: &[u8]) -> &[u8] {
    let first = settings.split(|&b| b == b'\n').next().unwrap_or_default();
    first.strip_suffix(b"\r").unwrap_or(first)
}

/// Which of [`IMAGE_FILES`] to write: the one `settings.ini` does not name.
pub fn image_slot(settings: &[u8]) -> usize {
    usize::from(first_line(settings).eq_ignore_ascii_case(FW_BASIS_LINES[0].as_bytes()))
}

/// The image `settings.ini` tells the bootloader to install, if it is one of ours. An update is
/// armed (ready, not yet installed) while that file still exists: the bootloader renames it
/// after installing. A new upload must not start then, or it would overwrite the armed
/// update's web folder.
pub fn named_image(settings: &[u8]) -> Option<&'static str> {
    let first = first_line(settings);
    FW_BASIS_LINES.iter().position(|l| first.eq_ignore_ascii_case(l.as_bytes())).map(|i| IMAGE_FILES[i])
}

/// `settings.ini` with its first line replaced by `FW_BASIS_LINES[slot]` (the bootloader reads
/// only the first 32 bytes); the other lines stay byte for byte. Returns the new length, or
/// `None` if `out` is too small.
pub fn switch_settings(existing: &[u8], slot: usize, out: &mut [u8]) -> Option<usize> {
    let rest = match existing.iter().position(|&b| b == b'\n') {
        Some(i) if existing.starts_with(b"FwBasis=") => &existing[i + 1..],
        _ if existing.starts_with(b"FwBasis=") => &[][..],
        _ => existing,
    };
    let line = FW_BASIS_LINES[slot].as_bytes();
    let n = line.len() + 1 + rest.len();
    let out = out.get_mut(..n)?;
    out[..line.len()].copy_from_slice(line);
    out[line.len()] = b'\n';
    out[line.len() + 1..].copy_from_slice(rest);
    Some(n)
}

#[cfg(test)]
mod tests;
