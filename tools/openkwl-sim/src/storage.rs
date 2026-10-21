//! The simulated SD card: a directory with the same layout and file formats as the unit's
//! card, read and written with `openkwl-config` exactly like the firmware will.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use openkwl_app::{App, Boot, PlatformInfo, Writes};
use openkwl_config::{
    AUTH_BAK, AUTH_BIN, CONFIG_BAK, CONFIG_INI, Config, DIR, RESET_PW, SECRETS_BAK, SECRETS_INI, auth_file, ini,
    state::{self, Loaded, RECORD_LEN},
};

pub struct Card {
    root: PathBuf,
    state: Loaded,
}

impl Card {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join(DIR))?;
        Ok(Self { root, state: Loaded::from_files(None, None) })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(DIR).join(name)
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        fs::read(self.path(name)).ok()
    }

    /// Loads everything like the firmware does at boot. `forget_password` simulates the
    /// `RESETPW` marker file.
    pub fn boot(&mut self, platform: PlatformInfo, forget_password: bool) -> io::Result<Boot> {
        if forget_password || self.root.join(RESET_PW).exists() {
            let _ = fs::remove_file(self.path(AUTH_BIN));
            let _ = fs::remove_file(self.path(AUTH_BAK));
            let _ = fs::remove_file(self.root.join(RESET_PW));
            println!("sd: password removed, setup mode");
        }
        let mut config_invalid = false;
        // As the firmware: an empty CONFIG.INI is a save cut off after its backup was written.
        let backup = || {
            self.read(CONFIG_BAK)
                .and_then(|b| ini::read_config(&String::from_utf8_lossy(&b), Config::default()).ok())
        };
        let config = match self.read(CONFIG_INI).map(|b| String::from_utf8_lossy(&b).into_owned()) {
            None => {
                println!("sd: no {CONFIG_INI}, using defaults");
                Config::default()
            }
            Some(text) if text.is_empty() && backup().is_some() => {
                println!("sd: {CONFIG_INI} was empty, restored from {CONFIG_BAK}");
                fs::copy(self.path(CONFIG_BAK), self.path(CONFIG_INI))?;
                backup().map_or_else(Config::default, |(c, _)| c)
            }
            Some(text) => match ini::read_config(&text, Config::default()) {
                Ok((c, report)) => {
                    for w in &report.warnings {
                        println!("sd: {CONFIG_INI} line {}: {:?}", w.line, w.kind);
                    }
                    fs::write(self.path(CONFIG_BAK), &text)?;
                    c
                }
                Err(e) => {
                    println!("sd: {CONFIG_INI} invalid ({e:?}), trying {CONFIG_BAK}");
                    config_invalid = true;
                    backup().map_or_else(Config::default, |(c, _)| c)
                }
            },
        };
        let secrets = [SECRETS_INI, SECRETS_BAK]
            .into_iter()
            .find_map(|f| {
                self.read(f)
                    .filter(|b| !b.is_empty())
                    .and_then(|b| ini::read_secrets(&String::from_utf8_lossy(&b)).ok())
            })
            .unwrap_or_default();
        // Missing means "set one"; present but unusable means "unknown" (`auth_file::UNKNOWN`).
        let files = [AUTH_BIN, AUTH_BAK].map(|f| self.read(f));
        let auth = files.iter().flatten().find_map(|b| auth_file::decode(b));
        let auth = if auth.is_none() && files.iter().any(Option::is_some) { Some(auth_file::UNKNOWN) } else { auth };
        let (a, b) = (self.read(state::Slot::A.file_name()), self.read(state::Slot::B.file_name()));
        self.state = Loaded::from_files(a.as_deref(), b.as_deref());
        for slot in [state::Slot::A, state::Slot::B] {
            if !self.path(slot.file_name()).exists() {
                fs::write(self.path(slot.file_name()), [0u8; RECORD_LEN])?;
            }
        }
        Ok(Boot { config, secrets, auth, state: self.state.state, config_invalid, platform })
    }

    /// The backup first, then the file, as the firmware (`openkwl_config::AUTH_BAK`).
    fn save(&self, name: &str, backup: &str, data: &[u8]) -> io::Result<()> {
        fs::write(self.path(backup), data)?;
        fs::write(self.path(name), data)
    }

    pub fn persist(&mut self, app: &App, w: &Writes) -> io::Result<()> {
        if w.config {
            let mut text = String::new();
            ini::write_config(app.config(), &mut text).map_err(io::Error::other)?;
            self.save(CONFIG_INI, CONFIG_BAK, text.as_bytes())?;
            println!("sd: wrote {CONFIG_INI}");
        }
        if w.secrets {
            let mut text = String::new();
            ini::write_secrets(app.secrets(), &mut text).map_err(io::Error::other)?;
            self.save(SECRETS_INI, SECRETS_BAK, text.as_bytes())?;
            println!("sd: wrote {SECRETS_INI}");
        }
        if w.auth {
            match app.auth_record() {
                Some(r) => self.save(AUTH_BIN, AUTH_BAK, &auth_file::encode(r))?,
                None => {
                    let _ = fs::remove_file(self.path(AUTH_BIN));
                    let _ = fs::remove_file(self.path(AUTH_BAK));
                }
            }
            println!("sd: wrote {AUTH_BIN}");
        }
        if let Some(s) = w.state {
            let (slot, record) = self.state.next_record(&s);
            fs::write(self.path(slot.file_name()), record)?;
            self.state.saved(s);
            println!("sd: wrote {}", slot.file_name());
        }
        Ok(())
    }
}

/// What the stock firmware left on the card: `FwBasis` of `settings.ini`. Read-only.
pub fn stock_details(sd: &Path) -> openkwl_api::StockDetails {
    use openkwl_config::stock;
    let ini = fs::read_to_string(sd.join(stock::SETTINGS_INI)).unwrap_or_default();
    openkwl_api::StockDetails {
        bootloader: Default::default(),
        fw_basis: stock::fw_basis(&ini).and_then(|f| f.try_into().ok()).unwrap_or_default(),
    }
}
