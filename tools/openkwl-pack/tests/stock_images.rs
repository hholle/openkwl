//! Checks against the stock Helios images. They are Helios property and not part of this
//! repository, so these tests are ignored by default. To run them, put the images in a folder
//! (`BASBACK.BIN` = official 2.24 from an original SD card; optionally `BAS00227.bin` = 2.27 as
//! downloaded in docs/INSTALL.md, and `BAS00221.bin`, `BAS00XXX.bin`, `BASBL110.bin` from an
//! original card) and run
//! `OPENKWL_STOCK_DIR=<folder> cargo test -p openkwl-pack -- --ignored`.
//! A missing optional image is reported and skipped.

use std::path::PathBuf;

use openkwl_pack::*;

fn stock(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var_os("OPENKWL_STOCK_DIR").expect("set OPENKWL_STOCK_DIR to the folder with the stock images");
    let path = PathBuf::from(dir).join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            eprintln!("SKIPPED: {} not readable ({e})", path.display());
            None
        }
    }
}

#[test]
#[ignore = "needs the stock images: OPENKWL_STOCK_DIR=<folder>"]
fn official_224_verifies_with_expected_crc() {
    let Some(img) = stock("BASBACK.BIN") else {
        return;
    };
    let info = verify(&img).expect("stock image verifies");
    assert_eq!(info.crc, 0x898b);
    assert_eq!(info.body_len, STOCK_PADDED_LEN);
    assert_eq!(info.initial_sp, 0x2001_aba8);
    assert_eq!(info.reset_vector, 0x0800_81b5);
}

#[test]
#[ignore = "needs the stock images: OPENKWL_STOCK_DIR=<folder>"]
fn repacking_reproduces_the_stock_image_byte_for_byte() {
    let Some(img) = stock("BASBACK.BIN") else {
        return;
    };
    let body = &img[..img.len() - TRAILER_LEN];
    // Strip the zero padding to get the "raw" binary, then pack it again.
    let raw_len = body.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let repacked = pack(&body[..raw_len], Some(STOCK_PADDED_LEN)).unwrap();
    assert_eq!(repacked, img);
}

#[test]
#[ignore = "needs the stock images: OPENKWL_STOCK_DIR=<folder>"]
fn other_stock_images() {
    for (name, crc) in [("BAS00221.bin", 0x2056), ("BAS00XXX.bin", 0xba4a), ("BAS00227.bin", 0x7514)] {
        let Some(img) = stock(name) else { continue };
        let info = verify(&img).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(info.crc, crc, "{name}");
    }
}

#[test]
#[ignore = "needs the stock images: OPENKWL_STOCK_DIR=<folder>"]
fn bootloader_image_is_not_an_application() {
    // BASBL110.bin is linked at 0x08000000, so the vector check must reject it.
    let Some(img) = stock("BASBL110.bin") else {
        return;
    };
    assert!(matches!(verify(&img), Err(Error::BadVector { .. }) | Err(Error::CrcMismatch { .. })));
}
