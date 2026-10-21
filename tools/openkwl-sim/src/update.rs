//! `PUT /api/v1/system/update` in the simulator: the same checks and the same files on the
//! simulated card as the firmware (`firmware/src/update.rs`, `storage.rs`), without the
//! install: the simulator has no bootloader.

use std::{fs, io::Read, path::Path};

use openkwl_update::{Event, IMAGE_FILES, MARKER, Reader, WEB_DIRS};

pub type Failure = (u16, &'static str, &'static str);
const NO_CARD: Failure = (503, "unavailable", "the SD card could not store the update");

pub fn receive(body: &mut impl Read, length: usize, sd: &Path) -> Result<(u16, u16), Failure> {
    let mut left = length;
    let r = store(body, &mut left, sd);
    // Take the rest after an error, so the client sees the answer (as the firmware does).
    if r.is_err() {
        let _ = std::io::copy(&mut body.take(left as u64), &mut std::io::sink());
    }
    r
}

fn store(body: &mut impl Read, left: &mut usize, sd: &Path) -> Result<(u16, u16), Failure> {
    let length = *left;
    if length > 2 * 1024 * 1024 {
        return Err((413, "payload_too_large", "the update file is too large"));
    }
    let settings_path = sd.join("settings.ini");
    let settings = fs::read(&settings_path).unwrap_or_default();
    // As the firmware: an armed update (named and still there) keeps its web folder.
    if openkwl_update::named_image(&settings).is_some_and(|image| sd.join(image).exists()) {
        return Err((409, "conflict", "an update is ready: restart the unit to install it first"));
    }
    let slot = openkwl_update::image_slot(&settings);
    // The simulator always serves `--www`; the update goes to the second folder like on a
    // unit that still serves the first.
    let web = sd.join(WEB_DIRS[1]);
    let mut reader = Reader::new();
    let mut file: Option<fs::File> = None;
    let mut buf = [0u8; 1536];
    while *left > 0 {
        let want = (*left).min(buf.len());
        let n = body.read(&mut buf[..want]).map_err(|_| (400, "bad_request", "the upload was cut off"))?;
        if n == 0 {
            return Err((400, "bad_request", "the upload was cut off"));
        }
        *left -= n;
        reader
            .push(&buf[..n], &mut |e| match e {
                Event::File { name, firmware, .. } => {
                    let path = if firmware {
                        sd.join(IMAGE_FILES[slot])
                    } else {
                        let _ = fs::create_dir_all(&web);
                        let _ = fs::remove_file(web.join(MARKER));
                        web.join(name)
                    };
                    file = fs::File::create(path).ok();
                    file.is_some()
                }
                Event::Data(d) => file.as_mut().is_some_and(|f| std::io::Write::write_all(f, d).is_ok()),
                Event::Done { .. } => true,
            })
            .map_err(|e| match e {
                openkwl_update::Error::Write => NO_CARD,
                e => (400, "bad_request", e.message()),
            })?;
    }
    let (crc, files) = reader.finished().ok_or((400, "bad_request", "the update file is incomplete"))?;
    if files > 0 {
        fs::write(web.join(MARKER), crc.to_le_bytes()).map_err(|_| NO_CARD)?;
    }
    let backup = sd.join("HELIOS/SETTINGS.BAK");
    if !backup.exists() {
        fs::create_dir_all(sd.join("HELIOS"))
            .and_then(|()| fs::write(&backup, &settings))
            .map_err(|_| NO_CARD)?;
    }
    let mut out = vec![0u8; settings.len() + 64];
    let n = openkwl_update::switch_settings(&settings, slot, &mut out).ok_or(NO_CARD)?;
    fs::write(&settings_path, &out[..n]).map_err(|_| NO_CARD)?;
    Ok((crc, files))
}
