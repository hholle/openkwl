//! SD card (SDIO) with FAT via `embedded-sdmmc`: settings and counters through `openkwl-config`
//! (same formats as `tools/openkwl-sim`), and the web UI from `WWW/`.
//!
//! The SDIO driver is async, `embedded-sdmmc` 0.10 is blocking: [`SdBlocks`] bridges them by
//! polling each block transfer to completion (`block_on`). That holds the executor for a few
//! milliseconds per block, which is acceptable for settings and static files. A later step can
//! move storage to its own interrupt-priority executor or an async FAT crate.
//!
//! Only `HELIOS/`, `WWW/` and the marker files in the root are ever touched; the stock files
//! on the card stay as they are (rollback, `docs/INSTALL.md`).

use core::{
    cell::{Cell, RefCell},
    fmt,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use defmt::{debug, info, warn};
use embassy_futures::block_on;
use embassy_stm32::{
    Peri,
    gpio::{Level, Output, Speed},
    peripherals::{DMA2_CH3, IWDG, PA15, PC8, PC9, PC10, PC11, PC12, PD2, SDIO},
    sdmmc::{
        self, Sdmmc,
        sd::{Card as SdCard, CmdBlock, DataBlock, StorageDevice},
    },
    time::Hertz,
    wdg::IndependentWatchdog,
};
use embassy_sync::{
    blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex},
    mutex::Mutex,
    signal::Signal,
};
use embassy_time::{Duration, Timer, with_timeout};
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, RawDirectory, RawFile, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use openkwl_app::{App, Boot, Writes, buf::SliceWriter};
use openkwl_config::{
    AUTH_BAK, AUTH_BIN, CONFIG_BAK, CONFIG_INI, CONFIG_MAX_LEN, Config, DIR, RESET_PW, SECRETS_BAK, SECRETS_INI, auth_file, ini,
    state::{self, Loaded, RECORD_LEN},
};
use static_cell::StaticCell;

use crate::{Irqs, SharedApp};

/// UTC seconds for FAT timestamps; updated by the control loop from the RTC (no 64-bit
/// atomics on Cortex-M3; u32 lasts until 2106).
pub static NOW_UNIX: AtomicU32 = AtomicU32::new(0);
/// Card size in MB, 0 without a card (`GET /system/details`).
static CAPACITY_MB: AtomicU32 = AtomicU32::new(0);

pub fn capacity_mb() -> Option<u32> {
    Some(CAPACITY_MB.load(Ordering::Relaxed)).filter(|&mb| mb > 0)
}

#[derive(Debug, Clone, Copy, defmt::Format)]
pub struct SdError;

impl fmt::Display for SdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SD card error")
    }
}

impl core::error::Error for SdError {}

/// Blocking `embedded-sdmmc` view of the async SDIO driver.
pub struct SdBlocks {
    dev: RefCell<StorageDevice<'static, 'static, SdCard>>,
}

impl BlockDevice for SdBlocks {
    type Error = SdError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), SdError> {
        let mut dev = self.dev.borrow_mut();
        let mut tmp = DataBlock::new();
        for (i, b) in blocks.iter_mut().enumerate() {
            block_on(dev.read_block(start.0 + i as u32, &mut tmp)).map_err(|_| SdError)?;
            b.contents.copy_from_slice(&tmp[..]);
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), SdError> {
        let mut dev = self.dev.borrow_mut();
        let mut tmp = DataBlock::new();
        for (i, b) in blocks.iter().enumerate() {
            tmp.copy_from_slice(&b.contents);
            block_on(dev.write_block(start.0 + i as u32, &tmp)).map_err(|_| SdError)?;
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, SdError> {
        let mut dev = self.dev.borrow_mut();
        let bytes = block_on(block_device_driver::BlockDevice::<512>::size(&mut *dev)).map_err(|_| SdError)?;
        Ok(BlockCount((bytes / 512) as u32))
    }
}

pub struct Clock;

impl TimeSource for Clock {
    fn get_timestamp(&self) -> Timestamp {
        let t = u64::from(NOW_UNIX.load(Ordering::Relaxed));
        let d = openkwl_core::clock::Date::from_days((t / 86_400) as u32);
        let s = (t % 86_400) as u32;
        Timestamp {
            year_since_1970: (d.year.saturating_sub(1970)).min(255) as u8,
            zero_indexed_month: d.month - 1,
            zero_indexed_day: d.day - 1,
            hours: (s / 3600) as u8,
            minutes: (s / 60 % 60) as u8,
            seconds: (s % 60) as u8,
        }
    }
}

/// Root and the web folder stay open; `HELIOS/` is opened per access, the other web folder
/// while an update fills it.
const MAX_DIRS: usize = 4;
/// One streaming web file per HTTP worker, plus one for settings and counters.
const MAX_FILES: usize = crate::http::WORKERS + 1;
type Volumes = VolumeManager<SdBlocks, Clock, MAX_DIRS, MAX_FILES, 1>;
const FREQ: Hertz = Hertz(24_000_000);

/// What stays open while the card works, so a request does not first re-read the partition
/// table and walk the root directory.
#[derive(Clone, Copy)]
struct Mount {
    root: RawDirectory,
    /// `None` if the card has no web folder (the web UI is missing).
    www: Option<RawDirectory>,
    /// Which of `openkwl_update::WEB_DIRS` is served.
    #[cfg_attr(not(feature = "update"), allow(dead_code))]
    web: usize,
}

pub struct Storage {
    /// `None` only for a moment while the card is re-initialised.
    vm: Option<Volumes>,
    mount: Option<Mount>,
    /// The web file each HTTP worker is streaming.
    www_files: [Option<RawFile>; crate::http::WORKERS],
    /// A firmware update being received: the web folder it fills, the open file, and which
    /// of `openkwl_update::IMAGE_FILES` takes the image; whether a card recovery dropped its
    /// handles (the upload fails, `update_close` clears it).
    #[cfg(feature = "update")]
    upload: (Option<RawDirectory>, Option<RawFile>, usize, bool),
    state: Loaded,
    prepared: Prepared,
}

/// `None` when no card was found at boot; everything then runs on defaults without saving.
pub type Card = Mutex<CriticalSectionRawMutex, Option<&'static mut Storage>>;

/// The one 12 KB buffer for `CONFIG.INI`: reading it at boot, writing it, and the HTTP export
/// and import. Lock order: `BIG`, then the card, then the app.
#[cfg(feature = "sd-log")]
const BOOT_LOG: &str = "BOOT.LOG";
/// The card's `settings.ini` before the first update from the web UI changed it.
#[cfg(feature = "update")]
const SETTINGS_BAK: &str = "SETTINGS.BAK";

pub static BIG: Mutex<CriticalSectionRawMutex, [u8; CONFIG_MAX_LEN]> = Mutex::new([0; CONFIG_MAX_LEN]);

/// The card stopped answering (removed, contact problem); the storage task re-initialises it.
static FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum Dir {
    Root,
    Helios,
}

type Directory<'a> = embedded_sdmmc::Directory<'a, SdBlocks, Clock, MAX_DIRS, MAX_FILES, 1>;
type FsResult<R> = Result<R, embedded_sdmmc::Error<SdError>>;

/// `Ok` as `Some`; a card error also marks the card as failed.
fn note<R>(r: FsResult<R>) -> Option<R> {
    match r {
        Ok(v) => Some(v),
        Err(embedded_sdmmc::Error::DeviceError(_)) => {
            if !FAILED.swap(true, Ordering::Relaxed) {
                warn!("sd: card error");
            }
            PRESENT.store(false, Ordering::Relaxed);
            None
        }
        Err(_) => None,
    }
}

/// The image CRC in a web folder's marker (`openkwl_update::MARKER`).
fn marker(vm: &Volumes, dir: RawDirectory) -> Option<u16> {
    let f = vm.open_file_in_dir(dir, openkwl_update::MARKER, Mode::ReadOnly).ok()?;
    let mut b = [0u8; 2];
    let n = vm.read(f, &mut b);
    let _ = vm.close_file(f);
    (n.ok()? == 2).then(|| u16::from_le_bytes(b))
}

/// A web file could not be opened.
pub enum WwwError {
    NotFound,
    /// Another worker is streaming the same file (the FAT library allows one handle per file).
    Busy,
}

impl Storage {
    fn new(blocks: SdBlocks) -> Self {
        let mut s = Self {
            vm: Some(VolumeManager::new(blocks, Clock)),
            mount: None,
            www_files: [None; crate::http::WORKERS],
            #[cfg(feature = "update")]
            upload: (None, None, 0, false),
            state: Loaded::from_files(None, None),
            prepared: Prepared::default(),
        };
        s.mount();
        s
    }

    fn mount(&mut self) -> bool {
        if self.mount.is_some() {
            return true;
        }
        let Some(vm) = self.vm.as_ref() else { return false };
        let Some(volume) = note(vm.open_raw_volume(VolumeIdx(0))) else { return false };
        let Some(root) = note(vm.open_root_dir(volume)) else {
            let _ = vm.close_volume(volume);
            return false;
        };
        // The second web folder only when its marker names the running firmware (an update
        // from the web UI put it there); otherwise `WWW/`, as `card.sh` writes it.
        let [first, second] = openkwl_update::WEB_DIRS;
        let (www, web) = match vm.open_dir(root, second) {
            Ok(d) if marker(vm, d) == Some(crate::board::image_crc()) => (Some(d), 1),
            other => {
                if let Ok(d) = other {
                    let _ = vm.close_dir(d);
                }
                (vm.open_dir(root, first).ok(), 0)
            }
        };
        self.mount = Some(Mount { root, www, web });
        true
    }

    /// Starts the card again after errors; every open handle is gone afterwards.
    async fn recover(&mut self) -> bool {
        self.www_files = [None; crate::http::WORKERS];
        self.mount = None;
        // An upload in progress loses its handles (the new volume manager hands out the same
        // IDs again): it fails instead of writing through stale ones.
        #[cfg(feature = "update")]
        {
            let lost = self.upload.0.is_some() || self.upload.1.is_some() || self.upload.3;
            self.upload = (None, None, 0, lost);
        }
        let Some(vm) = self.vm.take() else { return false };
        let (mut blocks, clock) = vm.free();
        let mut cmd_block = CmdBlock::new();
        let ok =
            matches!(with_timeout(Duration::from_secs(2), blocks.dev.get_mut().reacquire(&mut cmd_block, FREQ)).await, Ok(Ok(())));
        self.vm = Some(VolumeManager::new(blocks, clock));
        FAILED.store(false, Ordering::Relaxed);
        ok && self.mount() && !FAILED.load(Ordering::Relaxed)
    }

    /// Runs `f` in `dir`. The root stays open; `HELIOS/` is created when missing.
    fn with_dir<R>(&self, dir: Dir, f: impl FnOnce(&Directory<'_>) -> FsResult<R>) -> Option<R> {
        let vm = self.vm.as_ref()?;
        let m = self.mount?;
        match dir {
            Dir::Root => {
                let d = m.root.to_directory(vm);
                let r = f(&d);
                let _ = d.to_raw_directory(); // stays open
                note(r)
            }
            Dir::Helios => {
                let raw = match vm.open_dir(m.root, DIR) {
                    Ok(d) => d,
                    Err(embedded_sdmmc::Error::NotFound) => {
                        note(vm.make_dir_in_dir(m.root, DIR))?;
                        note(vm.open_dir(m.root, DIR))?
                    }
                    Err(e) => return note(Err(e)),
                };
                note(f(&raw.to_directory(vm))) // closed when dropped
            }
        }
    }

    /// Reads the start of a file into `buf`; returns how much was read.
    fn read(&self, dir: Dir, name: &str, buf: &mut [u8]) -> Option<usize> {
        self.read_whole(dir, name, buf).map(|(n, _)| n)
    }

    /// Reads a file into `buf`: how much was read, and whether that is all of it (`false`:
    /// the file is longer than `buf`, the text is cut off).
    fn read_whole(&self, dir: Dir, name: &str, buf: &mut [u8]) -> Option<(usize, bool)> {
        self.with_dir(dir, |d| {
            let f = d.open_file_in_dir(name, Mode::ReadOnly)?;
            let mut n = 0;
            while n < buf.len() {
                let r = f.read(&mut buf[n..])?;
                if r == 0 {
                    break;
                }
                n += r;
            }
            Ok((n, f.length() as usize <= n))
        })
    }

    /// Whether the file holds exactly `data` (compared in 512-byte pieces).
    fn same(&self, dir: Dir, name: &str, data: &[u8]) -> bool {
        self.with_dir(dir, |d| {
            let f = d.open_file_in_dir(name, Mode::ReadOnly)?;
            if f.length() as usize != data.len() {
                return Ok(false);
            }
            let mut piece = [0u8; 512];
            let mut at = 0;
            while at < data.len() {
                let n = f.read(&mut piece)?;
                if n == 0 || piece[..n] != data[at..at + n] {
                    return Ok(false);
                }
                at += n;
            }
            Ok(true)
        })
        .unwrap_or(false)
    }

    /// Saves a settings file in `HELIOS/` power-cut safe: the backup first, then the file
    /// (`openkwl_config::AUTH_BAK`). If the backup can't be written, the file is left as it is.
    fn save(&self, name: &str, backup: &str, data: &[u8]) -> bool {
        self.write(Dir::Helios, backup, data) && self.write(Dir::Helios, name, data)
    }

    fn write(&self, dir: Dir, name: &str, data: &[u8]) -> bool {
        self.with_dir(dir, |d| {
            let f = d.open_file_in_dir(name, Mode::ReadWriteCreateOrTruncate)?;
            f.write(data)?;
            f.close()
        })
        .is_some()
    }

    /// Overwrites a preallocated record in place (no FAT changes once the file exists).
    fn overwrite(&self, dir: Dir, name: &str, data: &[u8]) -> bool {
        self.with_dir(dir, |d| {
            let f = d.open_file_in_dir(name, Mode::ReadWriteCreateOrAppend)?;
            f.seek_from_start(0)?;
            f.write(data)?;
            f.close()
        })
        .is_some()
    }

    /// Writes a day record at `offset` in `HELIOS/DAYS.BIN` (`openkwl_app::history`), growing
    /// the file with zeros up to it the first time: one write a day, in place once a year ran.
    pub fn days_write(&self, offset: u32, record: &[u8]) -> bool {
        self.with_dir(Dir::Helios, |d| {
            let f = d.open_file_in_dir(openkwl_config::DAYS, Mode::ReadWriteCreateOrAppend)?;
            let zeros = [0u8; 512];
            while f.length() < offset {
                f.write(&zeros[..(offset - f.length()).min(512) as usize])?;
            }
            f.seek_from_start(offset)?;
            f.write(record)?;
            f.close()
        })
        .is_some()
    }

    /// Reads the record at `offset` of `HELIOS/DAYS.BIN` into `out`; `false` if there is none.
    pub fn days_read(&self, offset: u32, out: &mut [u8]) -> bool {
        self.with_dir(Dir::Helios, |d| {
            let f = d.open_file_in_dir(openkwl_config::DAYS, Mode::ReadOnly)?;
            if f.length() < offset + out.len() as u32 {
                return Ok(false);
            }
            f.seek_from_start(offset)?;
            let mut n = 0;
            while n < out.len() {
                match f.read(&mut out[n..])? {
                    0 => break,
                    r => n += r,
                }
            }
            Ok(n == out.len())
        })
        .unwrap_or(false)
    }

    fn delete(&self, dir: Dir, name: &str) -> bool {
        self.with_dir(dir, |d| d.delete_entry_in_dir(name)).is_some()
    }

    /// Appends what the debug log collected to `HELIOS/DEFMT.LOG` (`sdlog`).
    #[cfg(feature = "sd-log")]
    fn flush_log(&mut self) {
        const LOG: &str = "DEFMT.LOG";
        let mut piece = [0u8; 512];
        let (mut n, dropped) = crate::sdlog::take(&mut piece);
        if dropped > 0 {
            warn!("sd-log: {} messages dropped (ring full)", dropped);
        }
        if n == 0 {
            return;
        }
        self.with_dir(Dir::Helios, |d| {
            let mode = match d.open_file_in_dir(LOG, Mode::ReadOnly) {
                Ok(f) if f.length() >= crate::sdlog::FILE_MAX => Mode::ReadWriteCreateOrTruncate,
                _ => Mode::ReadWriteCreateOrAppend,
            };
            let f = d.open_file_in_dir(LOG, mode)?;
            while n > 0 {
                f.write(&piece[..n])?;
                n = crate::sdlog::take(&mut piece).0;
            }
            f.close()
        });
    }

    /// Appends one line to `HELIOS/BOOT.LOG` (restarted beyond 8 KB): how far each start got,
    /// readable on a computer without any tools. Only in the `sd-log` debug build; the normal
    /// build needs the flash for the firmware update.
    #[cfg(not(feature = "sd-log"))]
    #[inline(always)]
    pub fn boot_log(&self, _: fmt::Arguments<'_>) {}

    #[cfg(feature = "sd-log")]
    pub fn boot_log(&self, args: fmt::Arguments<'_>) {
        let mut line: heapless::String<96> = heapless::String::new();
        let ms = embassy_time::Instant::now().as_millis();
        let _ = fmt::write(&mut line, format_args!("{}.{:03} s  ", ms / 1000, ms % 1000));
        let _ = fmt::write(&mut line, args);
        let _ = line.push_str("\r\n");
        self.with_dir(Dir::Helios, |d| {
            let mode = match d.open_file_in_dir(BOOT_LOG, Mode::ReadOnly) {
                Ok(f) if f.length() >= 8 * 1024 => Mode::ReadWriteCreateOrTruncate,
                _ => Mode::ReadWriteCreateOrAppend,
            };
            let f = d.open_file_in_dir(BOOT_LOG, mode)?;
            f.write(line.as_bytes())?;
            f.close()
        });
    }

    /// Opens a web file for HTTP worker `worker` (closing what it had open); returns its length.
    pub fn www_open(&mut self, worker: usize, name: &str) -> Result<u32, WwwError> {
        self.www_close(worker);
        let (Some(vm), Some(www)) = (self.vm.as_ref(), self.mount.and_then(|m| m.www)) else {
            return Err(WwwError::NotFound);
        };
        let f = match vm.open_file_in_dir(www, name, Mode::ReadOnly) {
            Ok(f) => f,
            Err(embedded_sdmmc::Error::FileAlreadyOpen) => return Err(WwwError::Busy),
            Err(e) => {
                note(Err::<(), _>(e));
                return Err(WwwError::NotFound);
            }
        };
        match note(vm.file_length(f)) {
            Some(len) => {
                self.www_files[worker] = Some(f);
                Ok(len)
            }
            None => {
                let _ = vm.close_file(f);
                Err(WwwError::NotFound)
            }
        }
    }

    /// The next part of worker `worker`'s open web file; 0 at the end.
    pub fn www_read(&mut self, worker: usize, out: &mut [u8]) -> Option<usize> {
        let f = self.www_files[worker]?;
        note(self.vm.as_ref()?.read(f, out))
    }

    pub fn www_close(&mut self, worker: usize) {
        if let (Some(f), Some(vm)) = (self.www_files[worker].take(), self.vm.as_ref()) {
            let _ = vm.close_file(f);
        }
    }

    /// Checks and stores the next piece of an update package.
    #[cfg(feature = "update")]
    #[inline(never)]
    pub fn update_piece(&mut self, reader: &mut openkwl_update::Reader, data: &[u8]) -> Result<(), crate::update::Failure> {
        use openkwl_update::Event;
        reader
            .push(data, &mut |e| match e {
                Event::File { name, firmware, .. } => self.update_file(name, firmware),
                Event::Data(d) => self.update_write(d),
                Event::Done { .. } => true,
            })
            .map_err(|e| match e {
                openkwl_update::Error::Write => crate::update::NO_CARD,
                e => (400, "bad_request", e.message()),
            })
    }

    /// Ends an upload: arms a complete, checked package (`result` ok), and closes what it
    /// left open either way. The image CRC and the number of web files.
    #[cfg(feature = "update")]
    #[inline(never)]
    pub fn update_end(
        &mut self,
        result: Result<(), crate::update::Failure>,
        reader: &openkwl_update::Reader,
    ) -> Result<(u16, u16), crate::update::Failure> {
        let r = result.and_then(|()| reader.finished().ok_or((400, "bad_request", "the update file is incomplete")));
        let r = r.and_then(|(crc, files)| self.update_arm(crc).map(|()| (crc, files)));
        self.update_close();
        r
    }

    /// Whether an uploaded update waits for the restart that installs it: `settings.ini` names
    /// one of our images and it is still there (the bootloader renames it once installed).
    #[cfg(feature = "update")]
    pub fn update_armed(&self) -> bool {
        let mut settings = [0u8; 32];
        let n = self.read(Dir::Root, openkwl_config::stock::SETTINGS_INI, &mut settings).unwrap_or(0);
        openkwl_update::named_image(&settings[..n]).is_some_and(|image| self.read(Dir::Root, image, &mut [0u8; 1]).is_some())
    }

    /// Starts the next file of an update (`openkwl_update::Event::File`): the image in the
    /// root, web files in the web folder not in use, whose marker goes first.
    #[cfg(feature = "update")]
    fn update_file(&mut self, name: &str, firmware: bool) -> bool {
        let (Some(vm), Some(m), false) = (self.vm.as_ref(), self.mount, self.upload.3) else { return false };
        if let Some(f) = self.upload.1.take() {
            let _ = vm.close_file(f);
        }
        let dir = if firmware {
            let mut settings = [0u8; 32];
            let n = self.read(Dir::Root, openkwl_config::stock::SETTINGS_INI, &mut settings).unwrap_or(0);
            self.upload.2 = openkwl_update::image_slot(&settings[..n]);
            m.root
        } else {
            match self.upload.0 {
                Some(d) => d,
                None => {
                    let name = openkwl_update::WEB_DIRS[1 - m.web];
                    let d = match vm.open_dir(m.root, name) {
                        Ok(d) => Some(d),
                        Err(_) => note(vm.make_dir_in_dir(m.root, name)).and_then(|()| note(vm.open_dir(m.root, name))),
                    };
                    let Some(d) = d else { return false };
                    let _ = vm.delete_entry_in_dir(d, openkwl_update::MARKER);
                    self.upload.0 = Some(d);
                    d
                }
            }
        };
        let name = if firmware { openkwl_update::IMAGE_FILES[self.upload.2] } else { name };
        self.upload.1 = note(vm.open_file_in_dir(dir, name, Mode::ReadWriteCreateOrTruncate));
        self.upload.1.is_some()
    }

    #[cfg(feature = "update")]
    fn update_write(&mut self, data: &[u8]) -> bool {
        match (self.vm.as_ref(), self.upload.1) {
            (Some(vm), Some(f)) => note(vm.write(f, data)).is_some(),
            _ => false,
        }
    }

    /// Closes what an update left open.
    #[cfg(feature = "update")]
    fn update_close(&mut self) {
        self.upload.3 = false;
        let Some(vm) = self.vm.as_ref() else { return };
        if let Some(f) = self.upload.1.take() {
            let _ = vm.close_file(f);
        }
        if let Some(d) = self.upload.0.take() {
            let _ = vm.close_dir(d);
        }
    }

    /// Arms a complete, checked update: the marker for the web folder that goes with the image
    /// (the new one, or the current one for an image-only package), then `settings.ini`.
    #[cfg(feature = "update")]
    fn update_arm(&mut self, image_crc: u16) -> Result<(), crate::update::Failure> {
        use crate::update::NO_CARD;
        let (Some(vm), Some(m), false) = (self.vm.as_ref(), self.mount, self.upload.3) else { return Err(NO_CARD) };
        if let Some(f) = self.upload.1.take()
            && note(vm.close_file(f)).is_none()
        {
            return Err(NO_CARD);
        }
        // `settings.ini` first: one too long for the buffers is refused before anything changes
        // (it would be written back cut off).
        let mut old = [0u8; 512];
        let mut new = [0u8; 512];
        let (n, whole) = self.read_whole(Dir::Root, openkwl_config::stock::SETTINGS_INI, &mut old).ok_or(NO_CARD)?;
        let len = whole
            .then(|| openkwl_update::switch_settings(&old[..n], self.upload.2, &mut new))
            .flatten()
            .ok_or((409, "conflict", "settings.ini on the card is longer than 512 bytes"))?;
        let dir = self.upload.0.take().or(m.www);
        let marked = dir.is_none_or(|d| {
            let ok = vm.open_file_in_dir(d, openkwl_update::MARKER, Mode::ReadWriteCreateOrTruncate).and_then(|f| {
                vm.write(f, &image_crc.to_le_bytes())?;
                vm.close_file(f)
            });
            if Some(d) != m.www {
                let _ = vm.close_dir(d);
            }
            note(ok).is_some()
        });
        // The original settings.ini, kept once, for going back by hand (an empty copy, from a
        // write cut off by a power loss, does not count).
        if self.read(Dir::Helios, SETTINGS_BAK, &mut [0u8; 1]).is_none_or(|b| b == 0)
            && !self.write(Dir::Helios, SETTINGS_BAK, &old[..n])
        {
            return Err(NO_CARD);
        }
        if marked && self.write(Dir::Root, openkwl_config::stock::SETTINGS_INI, &new[..len]) { Ok(()) } else { Err(NO_CARD) }
    }

    /// Copies what `App::take_writes` asked for out of the app (quick, under the app lock).
    /// `CONFIG.INI` goes into `big`, which the caller keeps until [`Storage::flush`].
    fn prepare(&mut self, app: &App, w: &Writes, big: Option<&mut [u8]>) {
        let mut p = Prepared::default();
        if let (true, Some(big)) = (w.config, big) {
            let mut sw = SliceWriter::new(big);
            if ini::write_config(app.config(), &mut sw).is_ok() {
                p.config_len = Some(sw.len());
            } else {
                warn!("sd: {} does not fit", CONFIG_INI);
            }
        }
        if w.secrets {
            let mut text = heapless::String::new();
            if ini::write_secrets(app.secrets(), &mut text).is_ok() {
                p.secrets = Some(text);
            }
        }
        if w.auth {
            p.auth = Some(app.auth_record().map(auth_file::encode));
        }
        p.state = w.state;
        self.prepared = p;
    }

    /// Writes what [`Storage::prepare`] copied (slow, card I/O; no app lock needed).
    fn flush(&mut self, big: Option<&[u8]>) {
        let p = core::mem::take(&mut self.prepared);
        if let (Some(n), Some(big)) = (p.config_len, big) {
            if self.save(CONFIG_INI, CONFIG_BAK, &big[..n]) {
                debug!("sd: wrote {}", CONFIG_INI);
            } else {
                warn!("sd: writing {} failed", CONFIG_INI);
            }
        }
        if let Some(text) = p.secrets {
            self.save(SECRETS_INI, SECRETS_BAK, text.as_bytes());
        }
        match p.auth {
            Some(Some(record)) => {
                self.save(AUTH_BIN, AUTH_BAK, &record);
            }
            Some(None) => {
                self.delete(Dir::Helios, AUTH_BIN);
                self.delete(Dir::Helios, AUTH_BAK);
            }
            None => {}
        }
        if let Some(s) = p.state {
            let (slot, record) = self.state.next_record(&s);
            if self.overwrite(Dir::Helios, slot.file_name(), &record) {
                self.state.saved(s);
            }
        }
    }
}

/// Files copied out of the app, waiting to be written.
#[derive(Default)]
struct Prepared {
    /// `CONFIG.INI` text of this length is in [`BIG`].
    config_len: Option<usize>,
    secrets: Option<heapless::String<160>>,
    /// `Some(None)` = delete `AUTH.BIN` (password removed).
    auth: Option<Option<[u8; auth_file::LEN]>>,
    state: Option<openkwl_config::state::RuntimeState>,
}

/// Writes requested by the control loop, merged until the storage task gets to them.
static PENDING: BlockingMutex<CriticalSectionRawMutex, Cell<Writes>> =
    BlockingMutex::new(Cell::new(Writes { config: false, secrets: false, auth: false, state: None, day: false }));
static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Raised when a save completes and no further writes are pending, for restart and
/// power-fail (they reset it after their request, then wait).
pub static SAVED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// A card was found at boot (the control loop must not lock the card to ask).
pub static PRESENT: AtomicBool = AtomicBool::new(false);

/// Hands writes to the storage task; returns at once.
pub fn request(w: Writes) {
    if w == Writes::default() {
        return;
    }
    PENDING.lock(|p| {
        let mut all = p.get();
        all.config |= w.config;
        all.secrets |= w.secrets;
        all.auth |= w.auth;
        all.state = w.state.or(all.state);
        all.day |= w.day;
        p.set(all);
    });
    WAKE.signal(());
}

/// Does the SD writes in the normal-priority executor, so a slow card never delays the
/// control loop (which runs at a higher priority, see `main.rs`). Also brings the card back
/// after errors, and retries what could not be written.
#[embassy_executor::task]
pub async fn storage_task(app: &'static SharedApp, card: &'static Card) -> ! {
    let (mut said_running, mut said_network) = (false, false);
    loop {
        // Boot record: the normal executor runs, and later the network has an address.
        if !said_running || !said_network {
            let ip = crate::net::NETWORK.lock(|n| n.borrow().ip.clone());
            if let Some(s) = card.lock().await.as_deref_mut() {
                if !said_running {
                    s.boot_log(format_args!("running"));
                    said_running = true;
                }
                if let Some(ip) = ip {
                    s.boot_log(format_args!("network up, {}", ip));
                    said_network = true;
                } else if crate::phy::no_clock() {
                    s.boot_log(format_args!("network off: no 50 MHz reference clock"));
                    said_network = true;
                }
            }
        }
        #[cfg(not(feature = "sd-log"))]
        const ROUND_S: u64 = 10;
        #[cfg(feature = "sd-log")]
        const ROUND_S: u64 = crate::sdlog::FLUSH_S;
        let _ = with_timeout(Duration::from_secs(ROUND_S), WAKE.wait()).await;
        #[cfg(feature = "sd-log")]
        if !FAILED.load(Ordering::Relaxed)
            && let Some(s) = card.lock().await.as_deref_mut()
        {
            s.flush_log();
        }
        if FAILED.load(Ordering::Relaxed) {
            let mut g = card.lock().await;
            if let Some(s) = g.as_deref_mut()
                && s.recover().await
            {
                PRESENT.store(true, Ordering::Relaxed);
                debug!("sd: card is back");
            }
            continue;
        }
        // An import or export holds the big buffer while it talks to the browser: then
        // CONFIG.INI waits for the next round, and counters and the rest go now.
        let mut big = if PENDING.lock(|p| p.get().config || p.get().day) { BIG.try_lock().ok() } else { None };
        let w = PENDING.lock(|p| {
            let mut w = p.replace(Writes::default());
            if (w.config || w.day) && big.is_none() {
                p.set(Writes { config: w.config, day: w.day, ..Writes::default() });
                w.config = false;
                w.day = false;
            }
            w
        });
        if w == Writes::default() {
            continue;
        }
        let mut g = card.lock().await;
        if let Some(s) = g.as_deref_mut() {
            // The finished day first: the big buffer holds its record only for this.
            if w.day
                && let Some(b) = big.as_deref_mut()
                && let Some((offset, len)) = app.lock().await.history_finished(&mut b[..])
                && !s.days_write(offset, &b[..len])
            {
                warn!("sd: day record not written");
            }
            {
                let a = app.lock().await;
                s.prepare(&a, &w, big.as_deref_mut().map(|b| &mut b[..]));
            }
            let started = embassy_time::Instant::now();
            s.flush(big.as_deref().map(|b| &b[..]));
            debug!("sd: saved in {} ms", started.elapsed().as_millis());
            if FAILED.load(Ordering::Relaxed) {
                // Written again from the app's current state once the card is back.
                request(w);
            }
        }
        drop(g);
        drop(big);
        // Only once nothing is left: writes requested during this flush get their own round.
        if PENDING.lock(|p| p.get() == Writes::default()) {
            SAVED.signal(());
        }
    }
}

/// Switches the card on (PA15, active low) and
/// initialises SDIO (4-bit, DMA2 stream 3) and the FAT volume. As the stock firmware does.
#[allow(clippy::too_many_arguments)]
pub async fn init(
    sdio: Peri<'static, SDIO>,
    dma: Peri<'static, DMA2_CH3>,
    irqs: Irqs,
    clk: Peri<'static, PC12>,
    cmd: Peri<'static, PD2>,
    d0: Peri<'static, PC8>,
    d1: Peri<'static, PC9>,
    d2: Peri<'static, PC10>,
    d3: Peri<'static, PC11>,
    power: Peri<'static, PA15>,
    watchdog: &mut IndependentWatchdog<'static, IWDG>,
) -> &'static Card {
    static SDMMC: StaticCell<Sdmmc<'static>> = StaticCell::new();
    static CARD: StaticCell<Card> = StaticCell::new();
    let sdmmc = SDMMC.init(Sdmmc::new_4bit(sdio, dma, irqs, clk, cmd, d0, d1, d2, d3, sdmmc::Config::default()));
    let mut cmd_block = CmdBlock::new();

    // No card-detect pin: the stock firmware reads PH13, which the STM32F207VC (100 pins) does not
    // have; the card is simply tried.

    // Power: the stock firmware drives PA15 high (off), then low (on). If the card does not answer
    // that way, try the other polarity, so a wrong guess about the switch costs nothing.
    // A real power cycle (the bootloader left the card on and mid-transfer), time for the
    // card to settle, a time limit per attempt, and the stock polarity first and last: one
    // failed attempt must not leave the card switched off.
    let mut power = Output::new(power, Level::High, Speed::Low);
    let mut active_low = true;
    let mut found = false;
    for low in [true, true, false, true] {
        watchdog.pet();
        power.set_level(if low { Level::High } else { Level::Low }); // off
        Timer::after_millis(250).await;
        power.set_level(if low { Level::Low } else { Level::High }); // on
        Timer::after_millis(100).await;
        let attempt = with_timeout(Duration::from_secs(2), StorageDevice::new_sd_card(&mut *sdmmc, &mut cmd_block, FREQ)).await;
        if matches!(attempt, Ok(Ok(_))) {
            active_low = low;
            found = true;
            break;
        }
        warn!("sd: no answer with power active {}", if low { "low" } else { "high" });
    }
    core::mem::forget(power);

    watchdog.pet();
    let card = if found {
        with_timeout(Duration::from_secs(2), StorageDevice::new_sd_card(sdmmc, &mut cmd_block, FREQ))
            .await
            .ok()
    } else {
        None
    };
    match card.ok_or(()).and_then(|r| r.map_err(|_| ())) {
        Ok(dev) => {
            info!("sd: card ready (power active {})", if active_low { "low" } else { "high" });
            crate::sysinfo::board_report(|b| b.sd_power_active_low = Some(active_low));
            PRESENT.store(true, Ordering::Relaxed);
            let blocks = SdBlocks { dev: RefCell::new(dev) };
            if let Ok(n) = blocks.num_blocks() {
                CAPACITY_MB.store(n.0 / 2048, Ordering::Relaxed);
            }
            CARD.init(Mutex::new(Some(place_storage(blocks))))
        }
        Err(_) => {
            warn!("sd: no card answers, running on defaults without saving");
            CARD.init(Mutex::new(None))
        }
    }
}

/// The card's `Storage` in its static (about 1.5 KB; `CONFIG.INI` uses [`BIG`]).
#[inline(never)]
fn place_storage(blocks: SdBlocks) -> &'static mut Storage {
    static STORAGE: StaticCell<Storage> = StaticCell::new();
    STORAGE.init(Storage::new(blocks))
}

/// Loads everything like `tools/openkwl-sim` does.
/// `FwBasis` of the bootloader's `settings.ini`. Read-only.
fn stock_details(s: &mut Storage, buf: &mut [u8]) -> openkwl_api::StockDetails {
    use openkwl_config::stock;
    let fw_basis = s
        .read(Dir::Root, stock::SETTINGS_INI, buf)
        .and_then(|n| core::str::from_utf8(&buf[..n]).ok().and_then(stock::fw_basis).and_then(|f| f.try_into().ok()))
        .unwrap_or_default();
    openkwl_api::StockDetails { bootloader: crate::board::bootloader_version(), fw_basis }
}

pub async fn boot(card: &'static Card, platform: openkwl_app::PlatformInfo) -> Boot {
    let mut big = BIG.lock().await;
    let mut guard = card.lock().await;
    load(guard.as_deref_mut(), &mut big[..], platform)
}

/// The synchronous part of [`boot`], kept out of `main`'s async frame: the config parser and
/// the temporaries need several KB of stack only while they run.
#[inline(never)]
fn load(s: Option<&mut Storage>, buf: &mut [u8], mut platform: openkwl_app::PlatformInfo) -> Boot {
    let mut config_invalid = false;
    let Some(s) = s else {
        return Boot {
            config: Config::default(),
            secrets: Default::default(),
            auth: Some(auth_file::UNKNOWN),
            state: Default::default(),
            config_invalid: false,
            platform,
        };
    };

    s.boot_log(format_args!(
        "openKWL {} start, reset flags 0x{:02x}, card mounted",
        env!("CARGO_PKG_VERSION"),
        crate::sysinfo::RESET_FLAGS.load(Ordering::Relaxed)
    ));
    #[cfg(feature = "sd-log")]
    s.flush_log();
    platform.stock = stock_details(s, buf);
    if s.read(Dir::Root, RESET_PW, buf).is_some() {
        s.delete(Dir::Helios, AUTH_BIN);
        s.delete(Dir::Helios, AUTH_BAK);
        s.delete(Dir::Root, RESET_PW);
        debug!("sd: RESETPW found, password removed");
    }
    let mut config = Config::default();
    let parse = |text: &[u8]| {
        core::str::from_utf8(text)
            .ok()
            .and_then(|t| ini::read_config(t, Config::default()).ok())
            .map(|(c, _)| c)
    };
    // `read` gives `None` only when the file is missing or the card failed (`FAILED`). A file
    // longer than the buffer counts as damaged: its cut-off start must not load (or become
    // the backup).
    let read = s.read_whole(Dir::Helios, CONFIG_INI, buf).map(|(n, whole)| if whole { n } else { usize::MAX });
    let text = read.filter(|&n| n > 0 && n != usize::MAX && !FAILED.load(Ordering::Relaxed));
    match text.and_then(|n| parse(&buf[..n]).map(|c| (c, n))) {
        Some((c, n)) => {
            config = c;
            // Hand edits: the backup follows a CONFIG.INI that loads (written only when it
            // differs, SD write budget).
            if !s.same(Dir::Helios, CONFIG_BAK, &buf[..n]) {
                s.write(Dir::Helios, CONFIG_BAK, &buf[..n]);
            }
        }
        None if read.is_some() || FAILED.load(Ordering::Relaxed) => {
            let backup = s
                .read_whole(Dir::Helios, CONFIG_BAK, buf)
                .and_then(|(n, whole)| whole.then(|| parse(&buf[..n])).flatten().map(|c| (c, n)));
            // Empty: a save cut off by a power loss after the backup was written (`Storage::save`),
            // so the backup holds the latest settings: put them back, nothing to report. Damaged
            // (a hand edit) or unreadable: use the backup, say so, and leave both files alone.
            match backup {
                Some((c, n)) if read == Some(0) && !FAILED.load(Ordering::Relaxed) => {
                    config = c;
                    s.write(Dir::Helios, CONFIG_INI, &buf[..n]);
                    warn!("sd: {} was empty, restored from {}", CONFIG_INI, CONFIG_BAK);
                }
                backup => {
                    config_invalid = true;
                    warn!("sd: {} invalid, using {}", CONFIG_INI, CONFIG_BAK);
                    if let Some((c, _)) = backup {
                        config = c;
                    }
                }
            }
        }
        None => debug!("sd: no {}, using defaults", CONFIG_INI),
    }
    // An empty or damaged file (a save cut off by a power loss) falls back to its backup.
    let mut secrets = None;
    for name in [SECRETS_INI, SECRETS_BAK] {
        secrets = secrets.or_else(|| {
            let n = s.read(Dir::Helios, name, buf).filter(|&n| n > 0)?;
            core::str::from_utf8(&buf[..n]).ok().and_then(|t| ini::read_secrets(t).ok())
        });
    }
    let secrets = secrets.unwrap_or_default();
    // Missing means "set one" (first setup); present but unreadable means "unknown".
    let (mut auth, mut seen) = (None, FAILED.load(Ordering::Relaxed));
    for name in [AUTH_BIN, AUTH_BAK] {
        if auth.is_none()
            && let Some(n) = s.read(Dir::Helios, name, buf)
        {
            seen = true;
            auth = auth_file::decode(&buf[..n]);
        }
    }
    let auth = if auth.is_none() && (seen || FAILED.load(Ordering::Relaxed)) { Some(auth_file::UNKNOWN) } else { auth };
    let mut records = [[0u8; RECORD_LEN]; 2];
    let mut present = [false; 2];
    for (i, slot) in [state::Slot::A, state::Slot::B].into_iter().enumerate() {
        if let Some(n) = s.read(Dir::Helios, slot.file_name(), buf) {
            let n = n.min(RECORD_LEN);
            records[i][..n].copy_from_slice(&buf[..n]);
            present[i] = true;
        }
    }
    s.boot_log(format_args!("settings {}", if config_invalid { "invalid, using the backup or defaults" } else { "loaded" }));
    s.state = Loaded::from_files(present[0].then_some(&records[0][..]), present[1].then_some(&records[1][..]));
    Boot { config, secrets, auth, state: s.state.state, config_invalid, platform }
}

fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let mut mac = [0u8; 6];
    let mut parts = text.split(':');
    for b in &mut mac {
        *b = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    (parts.next().is_none() && mac != [0; 6] && mac != [0xFF; 6]).then_some(mac)
}

/// The factory MAC, written once by Helios into the chip's OTP area as ASCII
/// (`80:4B:20:…` at 0x1FFF7800, where the stock firmware reads it).
fn otp_mac() -> Option<[u8; 6]> {
    // SAFETY: OTP block 0 is always-readable system memory.
    let otp: [u8; 17] = unsafe { core::ptr::read_volatile(0x1FFF_7800 as *const [u8; 17]) };
    parse_mac(core::str::from_utf8(&otp).ok()?)
}

/// The factory MAC from OTP, else the one from the imported stock settings, else a locally
/// administered address derived from the chip's unique ID (never a fixed default).
pub fn mac_address(config: &Config) -> [u8; 6] {
    if let Some(m) = otp_mac().or_else(|| parse_mac(&config.device.mac)) {
        return m;
    }
    let uid = embassy_stm32::uid::uid();
    let mut m = [0x02, 0, 0, 0, 0, 0];
    for (i, b) in uid.iter().enumerate() {
        m[1 + i % 5] ^= b;
    }
    warn!("no factory MAC, using locally administered {:02x}", m);
    m
}
