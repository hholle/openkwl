//! `openkwl-pack`: verify or build SD-card update images for the stock bootloader.

use std::{fs, path::PathBuf, process::ExitCode};

use openkwl_pack::{STOCK_PADDED_LEN, file_name, pack, settings_ini, verify, web_files};

const USAGE: &str = "\
usage:
  openkwl-pack verify <image.bin>...
  openkwl-pack pack <raw.bin> --version <N> --out <dir> [--settings <settings.ini>] [--no-pad | --pad-to <bytes>]
  openkwl-pack bundle <image.bin> [<www dir>] --out <file.okwl>

pack takes a raw binary of an ELF linked at 0x08008000, e.g.
  rust-objcopy -O binary target/thumbv7m-none-eabi/release/openkwl-fw openkwl-fw.raw
and writes <dir>/BAS<NNNNN>.bin plus <dir>/settings.ini. By default the body is zero-padded to
0x37E00 bytes like the stock images. With --settings, only its FwBasis= line is changed.

bundle writes an update package for the web UI (Settings > Firmware update): the packed image
plus every file of the web UI folder (8.3 names, e.g. dist-sd/WWW). Without a folder the
package holds only the image and the unit keeps its web UI (e.g. to go back to the stock
firmware).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("verify") if args.len() > 1 => {
            let mut failed = false;
            for path in &args[1..] {
                let img = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                match verify(&img) {
                    Ok(i) => println!(
                        "{path}: OK  crc={:#06x} body={:#x} sp={:#010x} reset={:#010x}",
                        i.crc, i.body_len, i.initial_sp, i.reset_vector
                    ),
                    Err(e) => {
                        println!("{path}: FAIL  {e}");
                        failed = true;
                    }
                }
            }
            if failed { Err("verification failed".into()) } else { Ok(()) }
        }
        Some("pack") if args.len() > 1 => pack_cmd(&args[1], &args[2..]),
        Some("bundle") if args.len() > 1 => bundle_cmd(&args[1..]),
        _ => Err(USAGE.into()),
    }
}

fn pack_cmd(input: &str, opts: &[String]) -> Result<(), String> {
    let mut version = None;
    let mut out = None;
    let mut settings = None;
    let mut pad_to = Some(STOCK_PADDED_LEN);
    let mut it = opts.iter();
    while let Some(opt) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{opt} needs a value"));
        match opt.as_str() {
            "--version" => version = Some(value()?.parse::<u32>().map_err(|e| format!("--version: {e}"))?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--settings" => settings = Some(value()?.clone()),
            "--no-pad" => pad_to = None,
            "--pad-to" => {
                let v = value()?;
                let n = match v.strip_prefix("0x") {
                    Some(hex) => usize::from_str_radix(hex, 16),
                    None => v.parse(),
                };
                pad_to = Some(n.map_err(|e| format!("--pad-to: {e}"))?);
            }
            _ => return Err(format!("unknown option {opt}\n{USAGE}")),
        }
    }
    let version = version.ok_or("--version is required")?;
    let out = out.ok_or("--out is required")?;

    let raw = fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let img = pack(&raw, pad_to).map_err(|e| e.to_string())?;
    let name = file_name(version).map_err(|e| e.to_string())?;
    let existing = match &settings {
        Some(p) => Some(fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?),
        None => None,
    };

    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let image_path = out.join(&name);
    fs::write(&image_path, &img).map_err(|e| format!("{}: {e}", image_path.display()))?;
    let ini_path = out.join("settings.ini");
    fs::write(&ini_path, settings_ini(existing.as_deref(), &name)).map_err(|e| format!("{}: {e}", ini_path.display()))?;

    let info = verify(&img).map_err(|e| format!("self-check failed: {e}"))?;
    println!("wrote {} ({} bytes, crc={:#06x}) and {}", image_path.display(), img.len(), info.crc, ini_path.display());
    Ok(())
}

fn bundle_cmd(args: &[String]) -> Result<(), String> {
    let (mut inputs, mut out) = (Vec::new(), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = Some(it.next().ok_or("--out needs a value")?),
            _ => inputs.push(a),
        }
    }
    let out = out.ok_or("--out is required")?;
    let (image, www) = match inputs.as_slice() {
        [image] => (image, None),
        [image, www] => (image, Some(PathBuf::from(www))),
        _ => return Err(USAGE.into()),
    };
    let img = fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    verify(&img).map_err(|e| format!("{image}: {e}"))?;

    let mut files = vec![(openkwl_update::FIRMWARE.to_string(), img)];
    if let Some(dir) = www {
        files.extend(web_files(&dir)?);
    }

    let mut pkg = openkwl_update::MAGIC.to_vec();
    for (name, data) in &files {
        pkg.extend_from_slice(&openkwl_update::entry_header(name, data.len() as u32));
        pkg.extend_from_slice(data);
    }
    let crc = openkwl_update::crc16(0, &pkg);
    pkg.extend_from_slice(&openkwl_update::terminator(crc));

    // Self-check with the reader the unit uses.
    let mut r = openkwl_update::Reader::new();
    r.push(&pkg, &mut |_| true).map_err(|e| format!("self-check failed: {}", e.message()))?;
    if !r.done() {
        return Err("self-check failed: incomplete".into());
    }
    fs::write(out, &pkg).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out} ({} bytes: firmware + {} web files)", pkg.len(), files.len() - 1);
    Ok(())
}
