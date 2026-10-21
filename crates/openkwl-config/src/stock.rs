//! Read-only views of the stock files on the card (`settings.ini` of the bootloader). This
//! firmware never writes them; see `docs/INSTALL.md`.

/// Name of the bootloader's settings file in the card's root directory.
pub const SETTINGS_INI: &str = "settings.ini";

/// The `FwBasis=` entry of `settings.ini`: the main-controller image the bootloader installs,
/// e.g. `BAS00224.bin`.
pub fn fw_basis(settings_ini: &str) -> Option<&str> {
    settings_ini.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        k.trim().eq_ignore_ascii_case("FwBasis").then(|| v.trim()).filter(|v| !v.is_empty())
    })
}

/// A stock main-controller image (`BAS0xxxx.bin`, Helios numbering), as opposed to a
/// openKWL image (`BAS9xxxx.bin`) or the bootloader's `BASBACK.bin`.
pub fn is_stock_image(name: &str) -> bool {
    let n = name.as_bytes();
    n.len() == 12
        && n[..4].eq_ignore_ascii_case(b"BAS0")
        && n[4..8].iter().all(u8::is_ascii_digit)
        && name[8..].eq_ignore_ascii_case(".bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fw_basis() {
        let ini = "FwBasis=BAS00224.bin\r\nFwBecU1=BEC00207.bin\r\nerase=0\r\n";
        assert_eq!(fw_basis(ini), Some("BAS00224.bin"));
        assert_eq!(fw_basis("erase=0\n"), None);
        assert_eq!(fw_basis("FwBasis=\n"), None);
    }

    #[test]
    fn stock_image_names() {
        assert!(is_stock_image("BAS00224.bin"));
        assert!(is_stock_image("bas00227.BIN"));
        assert!(!is_stock_image("BAS90001.bin"));
        assert!(!is_stock_image("BASBACK.bin"));
        assert!(!is_stock_image("BAS00XXX.bin"));
    }
}
