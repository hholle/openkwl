use std::{vec, vec::Vec};

use super::*;

/// A minimal valid image: stack pointer, reset vector, the rest zero, CRC trailer.
fn image() -> Vec<u8> {
    let mut img = vec![0u8; IMAGE_BODY as usize];
    img[..4].copy_from_slice(&0x2001_FF00u32.to_le_bytes());
    img[4..8].copy_from_slice(&0x0800_8189u32.to_le_bytes());
    img[100] = 0x42;
    let crc = crc16(0, &img);
    img.extend_from_slice(&[crc as u8, (crc >> 8) as u8, 0, 0]);
    img
}

fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut p = MAGIC.to_vec();
    for (name, data) in entries {
        p.extend_from_slice(&entry_header(name, data.len() as u32));
        p.extend_from_slice(data);
    }
    let crc = crc16(0, &p);
    p.extend_from_slice(&terminator(crc));
    p
}

/// What a platform would store, as (name, bytes) plus the final event.
#[derive(Default, Debug)]
struct Stored {
    files: Vec<(std::string::String, Vec<u8>)>,
    done: Option<(u16, u16)>,
}

fn read(pkg: &[u8], piece: usize) -> (Result<(), Error>, Stored, bool) {
    let mut r = Reader::new();
    let mut st = Stored::default();
    let mut result = Ok(());
    for chunk in pkg.chunks(piece) {
        result = r.push(chunk, &mut |e| {
            match e {
                Event::File { name, .. } => st.files.push((name.into(), Vec::new())),
                Event::Data(d) => st.files.last_mut().unwrap().1.extend_from_slice(d),
                Event::Done { image_crc, web_files } => st.done = Some((image_crc, web_files)),
            }
            true
        });
        if result.is_err() {
            break;
        }
    }
    let done = r.done();
    (result, st, done)
}

#[test]
fn reads_a_package_in_any_piece_size() {
    let img = image();
    let pkg = package(&[(FIRMWARE, &img), ("INDEX.HTM", b"<html>"), ("APP.JS", b"x"), ("EMPTY.TXT", b"")]);
    let crc = u16::from_le_bytes([img[IMAGE_BODY as usize], img[IMAGE_BODY as usize + 1]]);
    for piece in [1, 7, 512, 1536, pkg.len()] {
        let (r, st, done) = read(&pkg, piece);
        assert_eq!(r, Ok(()), "piece {piece}");
        assert!(done);
        assert_eq!(st.done, Some((crc, 3)));
        assert_eq!(st.files.len(), 4);
        assert_eq!(st.files[0].1, img);
        assert_eq!(st.files[1], ("INDEX.HTM".into(), b"<html>".to_vec()));
    }
}

#[test]
fn a_firmware_only_package_keeps_the_web_ui() {
    let (r, st, _) = read(&package(&[(FIRMWARE, &image())]), 1000);
    assert_eq!(r, Ok(()));
    assert_eq!(st.done.unwrap().1, 0);
}

#[test]
fn incomplete_upload_is_not_done() {
    let pkg = package(&[(FIRMWARE, &image()), ("APP.JS", b"x")]);
    let (r, st, done) = read(&pkg[..pkg.len() - 1], 4096);
    assert_eq!(r, Ok(()));
    assert!(!done);
    assert_eq!(st.done, None);
}

#[test]
fn rejects_damage_and_wrong_contents() {
    let img = image();
    let good = package(&[(FIRMWARE, &img), ("APP.JS", b"abc")]);
    let first_err = |p: &[u8]| read(p, 333).0;

    assert_eq!(first_err(b"NOTAPKG1rest"), Err(Error::Magic));
    // A flipped bit in a web file: only the package CRC notices.
    let mut p = good.clone();
    let at = p.len() - 17;
    p[at] ^= 1;
    assert_eq!(first_err(&p), Err(Error::PackageCrc));
    // A flipped bit in the image: its own CRC notices before any web file.
    let mut p = good.clone();
    p[8 + 16 + 200] ^= 1;
    assert_eq!(first_err(&p), Err(Error::ImageCrc));
    let mut extra = good.clone();
    extra.push(0);
    assert_eq!(first_err(&extra), Err(Error::Trailing));
    assert_eq!(first_err(&package(&[("APP.JS", b"x")])), Err(Error::NoFirmware));
    assert_eq!(first_err(&package(&[(FIRMWARE, &img[..1000])])), Err(Error::NoFirmware));
    assert_eq!(first_err(&package(&[])), Err(Error::NoFirmware));
    assert_eq!(first_err(&package(&[(FIRMWARE, &img), ("app.js", b"x")])), Err(Error::Name));
    assert_eq!(first_err(&package(&[(FIRMWARE, &img), ("../X", b"x")])), Err(Error::Name));
    assert_eq!(first_err(&package(&[(FIRMWARE, &img), (MARKER, b"xx")])), Err(Error::Name));
    assert_eq!(first_err(&package(&[(FIRMWARE, &img), (FIRMWARE, &img)])), Err(Error::NoFirmware));
}

#[test]
fn image_rules_match_the_bootloader() {
    let with = |at: usize, v: u32| {
        let mut img = image();
        img[at..at + 4].copy_from_slice(&v.to_le_bytes());
        let crc = crc16(0, &img[..IMAGE_BODY as usize]);
        let t = IMAGE_BODY as usize;
        img[t] = crc as u8;
        img[t + 1] = (crc >> 8) as u8;
        read(&package(&[(FIRMWARE, &img)]), 4096).0
    };
    assert_eq!(with(0, 0x2001_FFEC), Ok(()));
    assert_eq!(with(0, 0x2001_FFF8), Err(Error::StackPointer), "the jump pops beyond RAM");
    assert_eq!(with(0, 0x2002_0000), Err(Error::StackPointer));
    assert_eq!(with(4, 0), Err(Error::Vector), "no reset vector");
    assert_eq!(with(4, 0x0800_8188), Err(Error::Vector), "not Thumb");
    assert_eq!(with(4, 0x0800_0101), Err(Error::Vector), "inside the bootloader");
    assert_eq!(with(12, 0x0804_0001), Err(Error::Vector), "beyond the slot");
}

#[test]
fn stops_when_the_card_fails() {
    let pkg = package(&[(FIRMWARE, &image())]);
    let mut r = Reader::new();
    assert_eq!(r.push(&pkg, &mut |e| !matches!(e, Event::Data(_))), Err(Error::Write));
}

#[test]
fn switches_only_the_first_line() {
    let mut out = [0u8; 256];
    let stock = b"FwBasis=BAS00221.bin\nFwBecU1=BEC00207.bin\r\nerase=0\n";
    let n = switch_settings(stock, 0, &mut out).unwrap();
    assert_eq!(&out[..n], b"FwBasis=BAS90001.bin\nFwBecU1=BEC00207.bin\r\nerase=0\n");
    let n = switch_settings(stock, 1, &mut out).unwrap();
    assert_eq!(&out[..n], b"FwBasis=BAS90002.bin\nFwBecU1=BEC00207.bin\r\nerase=0\n");
    // Without a FwBasis line the bootloader would ignore us: it goes first.
    let n = switch_settings(b"erase=0\n", 0, &mut out).unwrap();
    assert_eq!(&out[..n], b"FwBasis=BAS90001.bin\nerase=0\n");
    let n = switch_settings(b"FwBasis=BAS00221.bin", 0, &mut out).unwrap();
    assert_eq!(&out[..n], b"FwBasis=BAS90001.bin\n");
    assert_eq!(switch_settings(stock, 0, &mut [0u8; 10]), None);
    // The bootloader reads 32 bytes and cuts the name at the line end.
    assert!(FW_BASIS_LINES.iter().all(|l| l.len() < 32));
}

#[test]
fn never_writes_the_image_settings_ini_names() {
    // card.sh and stock cards: write the second name.
    assert_eq!(image_slot(b"FwBasis=BAS90001.bin\nerase=0\n"), 1);
    assert_eq!(image_slot(b"FwBasis=BAS90001.BIN\r\n"), 1);
    assert_eq!(image_slot(b"FwBasis=BAS90002.bin\n"), 0);
    assert_eq!(image_slot(b"FwBasis=BAS00221.bin\n"), 0);
    assert_eq!(image_slot(b""), 0);
}

#[test]
fn names() {
    for ok in ["INDEX.HTM", "APP.JS", "GEISTVAR.WF2", "OFL.TXT", "A", "A-B_1.X"] {
        assert!(valid_name(ok), "{ok}");
    }
    for bad in ["", ".JS", "TOOLONGNAME.JS", "A.ABCD", "a.js", "A B.JS", "A/B.JS", "A.B.C"] {
        assert!(!valid_name(bad), "{bad}");
    }
}

#[test]
fn too_many_entries_are_refused_not_wrapped() {
    // A valid image, then empty entries until the u16 counter would wrap and make a second
    // FIRMWARE.BIN count as the image again (which used to crash the reader).
    let img = image();
    let mut pkg = MAGIC.to_vec();
    pkg.extend_from_slice(&entry_header(FIRMWARE, img.len() as u32));
    pkg.extend_from_slice(&img);
    for _ in 0..u16::MAX {
        pkg.extend_from_slice(&entry_header("A", 0));
    }
    pkg.extend_from_slice(&entry_header(FIRMWARE, img.len() as u32));
    pkg.extend_from_slice(&img);
    let mut r = Reader::new();
    assert_eq!(r.push(&pkg, &mut |_| true), Err(Error::Name));
}

#[test]
fn knows_the_image_settings_ini_names() {
    assert_eq!(named_image(b"FwBasis=BAS90001.bin\r\nFwBecU1=BEC00207.bin\r\n"), Some("BAS90001.BIN"));
    assert_eq!(named_image(b"fwbasis=bas90002.BIN"), Some("BAS90002.BIN"));
    assert_eq!(named_image(b"FwBasis=BAS00227.bin\r\n"), None, "not ours");
    assert_eq!(named_image(b""), None);
}
