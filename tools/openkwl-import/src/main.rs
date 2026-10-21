//! `openkwl-import`: read a stock easyControls unit and write openKWL settings.

use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use openkwl_config::{DIR, STATE_A, STATE_B, ini, state};
use openkwl_import::{DirSource, Source, collect, import};

const USAGE: &str = "\
usage:
  openkwl-import --from http://helios/   --out <dir> [--cwp <file.CWP>] [--save-raw <dir>]
  openkwl-import --from-dir <captured>   --out <dir> [--cwp <file.CWP>]

Reads the settings of a unit running the stock firmware (read-only, no login) and writes
<out>/HELIOS/CONFIG.INI, STATEA.BIN and STATEB.BIN for the SD card. Run it before flashing.";

/// Minimal HTTP/1.0 client for the stock server: `POST /` with `xml=/data/<file>`.
struct HttpSource {
    host: String,
    port: u16,
}

impl HttpSource {
    fn new(url: &str) -> Result<Self, String> {
        let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
        let authority = rest.split('/').next().unwrap_or_default();
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().map_err(|_| format!("bad port in {url}"))?),
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err(format!("no host in {url}"));
        }
        Ok(Self { host: host.into(), port })
    }
}

impl Source for HttpSource {
    fn fetch(&mut self, file: &str) -> Result<Option<String>, String> {
        let body = format!("xml=/data/{file}");
        let request = format!(
            "POST / HTTP/1.0\r\nHost: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            body.len()
        );
        let mut stream = TcpStream::connect((self.host.as_str(), self.port)).map_err(|e| format!("{}: {e}", self.host))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).map_err(|e| e.to_string())?;
        stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|e| format!("{file}: {e}"))?;
        let text = String::from_utf8_lossy(&response);
        let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| format!("{file}: malformed response"))?;
        if !head.starts_with("HTTP/1.0 200") && !head.starts_with("HTTP/1.1 200") {
            return Ok(None);
        }
        // Unknown files come back as the login page.
        Ok(body.contains("<PARAMETER>").then(|| body.to_owned()))
    }
}

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn run(args: &[String]) -> Result<(), String> {
    let mut source: Option<Box<dyn Source>> = None;
    let (mut out, mut cwp, mut save_raw) = (None, None, None);
    let mut it = args.iter();
    while let Some(opt) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{opt} needs a value\n{USAGE}"));
        match opt.as_str() {
            "--from" => source = Some(Box::new(HttpSource::new(&value()?)?)),
            "--from-dir" => source = Some(Box::new(DirSource(PathBuf::from(value()?)))),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--cwp" => cwp = Some(PathBuf::from(value()?)),
            "--save-raw" => save_raw = Some(PathBuf::from(value()?)),
            _ => return Err(USAGE.into()),
        }
    }
    let mut source = source.ok_or(USAGE)?;
    let out = out.ok_or(USAGE)?;

    let stock = collect(source.as_mut())?;
    if let Some(dir) = save_raw {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (file, xml) in &stock.raw {
            write(&dir.join(file), xml.as_bytes())?;
        }
    }
    let cwp_bytes = match cwp {
        Some(p) => Some(fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?),
        None => None,
    };
    let result = import(&stock, cwp_bytes.as_deref());

    let dir = out.join(DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut ini_text = String::new();
    ini::write_config(&result.config, &mut ini_text).map_err(|e| e.to_string())?;
    write(&dir.join(openkwl_config::CONFIG_INI), ini_text.as_bytes())?;
    // Slot A holds the imported counters; B is preallocated so later saves never grow a file.
    let loaded = state::Loaded::from_files(None, None);
    let (_, record) = loaded.next_record(&result.state);
    write(&dir.join(STATE_A), &record)?;
    write(&dir.join(STATE_B), &[0u8; state::RECORD_LEN])?;

    println!("wrote {} ({} bytes), {STATE_A}, {STATE_B}", dir.join(openkwl_config::CONFIG_INI).display(), ini_text.len());
    println!("{} parameters, {} schedule files read", stock.values.len(), stock.profiles.iter().flatten().flatten().count());
    for note in &result.notes {
        println!("note: {}", note.0);
    }
    Ok(())
}
