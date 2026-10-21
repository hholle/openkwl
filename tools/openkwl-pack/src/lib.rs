//! Update images for the stock Helios bootloader (its rules: `firmware/memory.x`, `docs/UPDATING.md`).
//!
//! An application image is the raw flash contents from `0x08008000`, followed by a 4-byte
//! trailer: CRC-16/XMODEM over everything before it, little-endian, then two zero bytes.
//! All stock application images (`BASBACK.BIN`, `BAS00221.bin`, `BAS00XXX.bin`) are exactly
//! [`STOCK_PADDED_LEN`] bytes plus the trailer, zero-padded, so [`pack`] pads to that size by
//! default. Whether the bootloader requires it is not confirmed (README bring-up checklist).
//!
//! The bootloader flashes the file named by `FwBasis=` in `settings.ini` on the SD card.

use std::fmt;

/// Flash address the application is linked at (after the 32 KB bootloader).
pub const APP_BASE: u32 = 0x0800_8000;
/// End of the largest STM32F2x7 flash (1 MB). The real chip may be smaller.
pub const FLASH_END: u32 = 0x0810_0000;
/// SRAM of the STM32F2x7 (128 KB).
/// The stock bootloader only jumps to an image whose initial stack pointer satisfies
/// `(sp & 0x2FFF_0000) == 0x2001_0000`, i.e. lies in 0x2001_0000–0x2001_FFFF
/// (the stock bootloader's observed behaviour). 0x2002_0000, the end of RAM, is refused.
///
/// Its jump also reads 16 bytes from the new stack before branching, so the stack pointer
/// must leave at least that much below the end of RAM: 0x2001_FFF8 faulted inside the bootloader (dark board).
pub const SRAM_START: u32 = 0x2001_0000;
pub const SRAM_END: u32 = 0x2001_FFFF - 16;
/// Length of every stock application image without its trailer.
pub const STOCK_PADDED_LEN: usize = 0x37E00;
pub const TRAILER_LEN: usize = 4;
/// Vector table entries checked: initial SP, reset and the 14 system exception slots.
const CHECKED_VECTORS: usize = 16;

/// CRC-16/XMODEM: poly 0x1021, init 0, no reflection, no final XOR.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooShort(usize),
    /// Trailer bytes 2–3 are not zero.
    BadTrailer([u8; 4]),
    CrcMismatch {
        stored: u16,
        computed: u16,
    },
    BadStackPointer(u32),
    /// Reset or exception vector not a Thumb address inside the application flash.
    BadVector {
        index: usize,
        value: u32,
    },
    TooLarge {
        len: usize,
        max: usize,
    },
    /// The input already ends in a valid trailer; packing it again would nest trailers.
    AlreadyPacked,
    BadVersion(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort(n) => write!(f, "image too short ({n} bytes)"),
            Error::BadTrailer(t) => write!(f, "trailer {t:02x?} does not end in 00 00"),
            Error::CrcMismatch { stored, computed } => {
                write!(f, "CRC mismatch: trailer says {stored:#06x}, contents give {computed:#06x}")
            }
            Error::BadStackPointer(sp) => {
                write!(f, "initial stack pointer {sp:#010x}: the stock bootloader only boots 0x20010000–0x2001FFFF")
            }
            Error::BadVector { index, value } => write!(
                f,
                "vector {index} = {value:#010x} is not a Thumb address in {APP_BASE:#010x}..{FLASH_END:#010x} \
                 (linked at the wrong address?)"
            ),
            Error::TooLarge { len, max } => write!(f, "image is {len} bytes, limit is {max}"),
            Error::AlreadyPacked => write!(f, "input already has a valid CRC trailer; pass the raw binary"),
            Error::BadVersion(v) => write!(f, "version {v} does not fit the 5-digit file name"),
        }
    }
}

impl std::error::Error for Error {}

/// Facts about a verified image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub crc: u16,
    /// Length without the trailer.
    pub body_len: usize,
    pub initial_sp: u32,
    pub reset_vector: u32,
}

fn word(img: &[u8], index: usize) -> u32 {
    let o = index * 4;
    u32::from_le_bytes([img[o], img[o + 1], img[o + 2], img[o + 3]])
}

/// Checks that `body` (no trailer) starts with a Cortex-M vector table for [`APP_BASE`].
pub fn check_vectors(body: &[u8]) -> Result<(u32, u32), Error> {
    if body.len() < CHECKED_VECTORS * 4 {
        return Err(Error::TooShort(body.len()));
    }
    let sp = word(body, 0);
    if !(SRAM_START..=SRAM_END).contains(&sp) || !sp.is_multiple_of(4) {
        return Err(Error::BadStackPointer(sp));
    }
    let end = APP_BASE + body.len() as u32;
    for index in 1..CHECKED_VECTORS {
        let value = word(body, index);
        // Reserved slots are zero; vectors 2–15 may be zero when unused.
        if value == 0 && index != 1 {
            continue;
        }
        if value & 1 == 0 || !(APP_BASE..end).contains(&(value & !1)) {
            return Err(Error::BadVector { index, value });
        }
    }
    Ok((sp, word(body, 1)))
}

/// Verifies trailer, CRC and vector table of a packed image.
pub fn verify(img: &[u8]) -> Result<ImageInfo, Error> {
    if img.len() < TRAILER_LEN + CHECKED_VECTORS * 4 {
        return Err(Error::TooShort(img.len()));
    }
    let (body, trailer) = img.split_at(img.len() - TRAILER_LEN);
    if trailer[2..] != [0, 0] {
        return Err(Error::BadTrailer(trailer.try_into().unwrap()));
    }
    let stored = u16::from_le_bytes([trailer[0], trailer[1]]);
    let computed = crc16_xmodem(body);
    if stored != computed {
        return Err(Error::CrcMismatch { stored, computed });
    }
    let (initial_sp, reset_vector) = check_vectors(body)?;
    Ok(ImageInfo { crc: computed, body_len: body.len(), initial_sp, reset_vector })
}

/// Builds a packed image from a raw binary (`objcopy -O binary` of an ELF linked at
/// [`APP_BASE`]). With `pad_to`, the body is zero-padded to exactly that length.
pub fn pack(raw: &[u8], pad_to: Option<usize>) -> Result<Vec<u8>, Error> {
    if verify(raw).is_ok() {
        return Err(Error::AlreadyPacked);
    }
    check_vectors(raw)?;
    let max = pad_to.unwrap_or((FLASH_END - APP_BASE) as usize);
    if raw.len() > max {
        return Err(Error::TooLarge { len: raw.len(), max });
    }
    let mut img = raw.to_vec();
    if let Some(len) = pad_to {
        img.resize(len, 0);
    }
    let crc = crc16_xmodem(&img);
    img.extend_from_slice(&crc.to_le_bytes());
    img.extend_from_slice(&[0, 0]);
    Ok(img)
}

/// 8.3 file name the bootloader expects, e.g. `BAS00300.bin`.
pub fn file_name(version: u32) -> Result<String, Error> {
    if version > 99_999 {
        return Err(Error::BadVersion(version));
    }
    Ok(format!("BAS{version:05}.bin"))
}

/// `settings.ini` naming `image`. With `existing`, only the `FwBasis=` line is replaced
/// (or added first), so the module entries (`FwBecU1=` …) and `erase=` stay as they were.
/// Line endings are LF like the stock file.
pub fn settings_ini(existing: Option<&str>, image: &str) -> String {
    let line = format!("FwBasis={image}");
    let Some(existing) = existing else {
        return format!("{line}\nerase=0\n");
    };
    let mut out = String::new();
    let mut replaced = false;
    for l in existing.lines() {
        if l.trim_start().starts_with("FwBasis=") {
            if !replaced {
                out.push_str(&line);
                out.push('\n');
                replaced = true;
            }
        } else {
            out.push_str(l.trim_end_matches('\r'));
            out.push('\n');
        }
    }
    if !replaced {
        out.insert_str(0, &format!("{line}\n"));
    }
    out
}

/// The files of a web UI folder for an update package, sorted, as `(packaged name, contents)`.
/// The packaged name is upper-case (the card's FAT 8.3 names); the file is read under its own
/// name, so a lower-case folder works on case-sensitive file systems too.
pub fn web_files(dir: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().to_uppercase(), e.path()))
        .collect();
    entries.sort();
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(entries.len());
    for (name, path) in entries {
        if name.len() > 12 {
            return Err(format!("{name}: not an 8.3 name"));
        }
        if files.last().is_some_and(|(prev, _)| *prev == name) {
            return Err(format!("{name}: two files with this name"));
        }
        let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push((name, data));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid body: SP, reset handler, NMI, HardFault, rest zero.
    fn raw(len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len];
        for (i, w) in [0x2001_0000u32, APP_BASE + 0x101, APP_BASE + 0x201, APP_BASE + 0x201].iter().enumerate() {
            v[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        v
    }

    #[test]
    fn crc_check_value() {
        // Standard CRC catalogue check value for CRC-16/XMODEM.
        assert_eq!(crc16_xmodem(b"123456789"), 0x31c3);
        assert_eq!(crc16_xmodem(b""), 0);
    }

    #[test]
    fn stack_pointer_rule_of_the_bootloader() {
        let mut body = vec![0u8; CHECKED_VECTORS * 4];
        body[4..8].copy_from_slice(&(APP_BASE + 0x21).to_le_bytes());
        // 0x2001_FFF8 passes the bootloader's check but faults in its jump (it pops from the new stack).
        for (sp, ok) in [
            (0x2001_FF00u32, true),
            (0x2001_FFEC, true),
            (0x2001_FFF8, false),
            (0x2001_0000, true),
            (0x2002_0000, false),
            (0x2000_8000, false),
        ] {
            body[..4].copy_from_slice(&sp.to_le_bytes());
            assert_eq!(check_vectors(&body).is_ok(), ok, "{sp:#x}");
        }
    }

    #[test]
    fn pack_then_verify() {
        let img = pack(&raw(1000), None).unwrap();
        assert_eq!(img.len(), 1004);
        assert_eq!(&img[1002..], &[0, 0]);
        let info = verify(&img).unwrap();
        assert_eq!(info.body_len, 1000);
        assert_eq!(info.reset_vector, APP_BASE + 0x101);
        assert_eq!(pack(&img, None), Err(Error::AlreadyPacked));
    }

    #[test]
    fn padding() {
        let img = pack(&raw(1000), Some(STOCK_PADDED_LEN)).unwrap();
        assert_eq!(img.len(), STOCK_PADDED_LEN + TRAILER_LEN);
        assert!(verify(&img).is_ok());
        assert_eq!(
            pack(&raw(STOCK_PADDED_LEN + 1), Some(STOCK_PADDED_LEN)),
            Err(Error::TooLarge { len: STOCK_PADDED_LEN + 1, max: STOCK_PADDED_LEN })
        );
    }

    #[test]
    fn detects_corruption() {
        let mut img = pack(&raw(1000), None).unwrap();
        img[500] ^= 1;
        assert!(matches!(verify(&img), Err(Error::CrcMismatch { .. })));
        let mut img = pack(&raw(1000), None).unwrap();
        img[1003] = 1;
        assert!(matches!(verify(&img), Err(Error::BadTrailer(_))));
    }

    #[test]
    fn rejects_wrong_link_address() {
        let mut r = raw(1000);
        // Reset handler linked for 0x08000000 (no bootloader offset).
        r[4..8].copy_from_slice(&0x0800_0101u32.to_le_bytes());
        assert_eq!(pack(&r, None), Err(Error::BadVector { index: 1, value: 0x0800_0101 }));
        let mut r = raw(1000);
        r[4..8].copy_from_slice(&(APP_BASE + 0x100).to_le_bytes());
        assert!(matches!(pack(&r, None), Err(Error::BadVector { index: 1, .. })), "ARM-mode address");
        let mut r = raw(1000);
        r[0..4].copy_from_slice(&0x2003_0000u32.to_le_bytes());
        assert_eq!(pack(&r, None), Err(Error::BadStackPointer(0x2003_0000)));
        assert_eq!(pack(&[0; 10], None), Err(Error::TooShort(10)));
    }

    #[test]
    fn names() {
        assert_eq!(file_name(300).unwrap(), "BAS00300.bin");
        assert_eq!(file_name(100_000), Err(Error::BadVersion(100_000)));
    }

    #[test]
    fn settings() {
        assert_eq!(settings_ini(None, "BAS00300.bin"), "FwBasis=BAS00300.bin\nerase=0\n");
        let stock = "FwBasis=BAS00224.bin\nFwBecU1=BEC00207.bin\nFwErwMd=ERW00200.bin\nFwExSen=EXS00204.bin\nerase=0\n";
        assert_eq!(
            settings_ini(Some(stock), "BAS00300.bin"),
            "FwBasis=BAS00300.bin\nFwBecU1=BEC00207.bin\nFwErwMd=ERW00200.bin\nFwExSen=EXS00204.bin\nerase=0\n"
        );
        assert_eq!(settings_ini(Some("erase=0\r\n"), "BAS00300.bin"), "FwBasis=BAS00300.bin\nerase=0\n");
    }

    #[test]
    fn web_files_read_lower_case_names() {
        let dir = std::env::temp_dir().join(format!("openkwl-pack-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("index.htm"), b"<html>").unwrap();
        std::fs::write(dir.join("APP.JS"), b"js").unwrap();
        let files = web_files(&dir);
        std::fs::write(dir.join("a-very-long-name.js"), b"").unwrap();
        let long = web_files(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        let files = files.unwrap();
        let names: Vec<_> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        assert_eq!(names, [("APP.JS", &b"js"[..]), ("INDEX.HTM", &b"<html>"[..])], "folders skipped, names upper-cased");
        assert!(long.unwrap_err().contains("8.3"));
    }
}
