//! `openkwl-sim`: runs the firmware's application layer (`openkwl-app`) on the host.
//!
//! The same code as on the unit handles the REST API, runs the control loop and decides
//! what to save; this binary supplies time, randomness, a thermal model of the unit and a
//! directory standing in for the SD card.

mod http;
mod plant;
mod storage;
mod update;

use std::{
    net::TcpListener,
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use openkwl_app::{App, Connectivity, PlatformInfo};
use plant::Plant;
use storage::Card;

const USAGE: &str = "\
usage: openkwl-sim [options]

  --sd <dir>          simulated SD card (default: sim-sd); put an imported HELIOS/CONFIG.INI here
  --port <n>          HTTP port (default: 8080)
  --www <dir>         serve the web UI build from here (e.g. web/dist)
  --speed <n>         simulated seconds per real second (default: 1)
  --outdoor <°C>      mean outdoor temperature (default: 12)
  --fixed-outdoor     keep the outdoor temperature constant instead of a daily curve
  --icing             let the heat exchanger ice up below -3 °C (try --outdoor -8)
  --no-clock          start without a valid clock (as before the first NTP sync)
  --setup             forget the password and start in setup mode

The API is at http://localhost:<port>/api/v1; with --www the UI is at http://localhost:<port>/.
For UI development run `pnpm dev` in web/ instead; it proxies /api to this simulator.";

struct Options {
    sd: PathBuf,
    port: u16,
    www: Option<PathBuf>,
    speed: u32,
    outdoor: f32,
    fixed_outdoor: bool,
    icing: bool,
    clock: bool,
    setup: bool,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        sd: "sim-sd".into(),
        port: 8080,
        www: None,
        speed: 1,
        outdoor: 12.0,
        fixed_outdoor: false,
        icing: false,
        clock: true,
        setup: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--sd" => o.sd = value()?.into(),
            "--port" => o.port = value()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--www" => o.www = Some(value()?.into()),
            "--speed" => o.speed = value()?.parse::<u32>().map_err(|e| format!("--speed: {e}"))?.max(1),
            "--outdoor" => o.outdoor = value()?.parse().map_err(|e| format!("--outdoor: {e}"))?,
            "--fixed-outdoor" => o.fixed_outdoor = true,
            "--icing" => o.icing = true,
            "--no-clock" => o.clock = false,
            "--setup" => o.setup = true,
            "-h" | "--help" => return Err(USAGE.into()),
            _ => return Err(format!("unknown option {a}\n\n{USAGE}")),
        }
    }
    Ok(o)
}

fn platform(sd: &std::path::Path) -> PlatformInfo {
    use openkwl_api::system::*;
    fn s<const N: usize>(v: &str) -> heapless::String<N> {
        v.try_into().unwrap_or_default()
    }
    PlatformInfo {
        firmware: FirmwareDetails {
            version: s(env!("CARGO_PKG_VERSION")),
            build: s("simulator"),
            platform: s("simulator"),
            target: s(std::env::consts::ARCH),
            rustc: s(env!("OPENKWL_RUSTC")),
            profile: s(if cfg!(debug_assertions) { "debug" } else { "release" }),
            built_unix_s: env!("OPENKWL_BUILT").parse().ok(),
            uptime_s: 0,
        },
        chip: ChipDetails {
            mcu: s("host simulator"),
            sysclk_hz: 120_000_000,
            reset_reason: openkwl_api::ResetReason::PowerOn,
            ..Default::default()
        },
        memory: MemoryDetails { flash_slot_bytes: 0x37E00, ram_bytes: 128 * 1024, ..Default::default() },
        stock: storage::stock_details(sd),
        rtc_clock: s("host"),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let o = options()?;
    let mut card = Card::new(o.sd.clone()).map_err(|e| format!("{}: {e}", o.sd.display()))?;
    let boot = card.boot(platform(&o.sd), o.setup).map_err(|e| e.to_string())?;
    let app = Arc::new(Mutex::new(App::new(boot)));

    let listener = TcpListener::bind(("127.0.0.1", o.port)).map_err(|e| format!("port {}: {e}", o.port))?;
    println!("openkwl-sim: http://localhost:{}/api/v1 (SD card: {})", o.port, o.sd.display());
    if let Some(w) = &o.www {
        println!("openkwl-sim: web UI from {} at http://localhost:{}/", w.display(), o.port);
    }
    {
        let (app, www, sd) = (Arc::clone(&app), o.www.clone(), o.sd.clone());
        thread::spawn(move || http::serve(listener, app, www, sd));
    }

    // Simulated clock: starts at the host's time, runs `speed` times faster.
    let start_unix = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let mut sim_seconds: u64 = 0;
    let mut clock_offset: i64 = 0;
    let mut clock_valid = o.clock;
    let mut plant = Plant::new(o.outdoor, o.fixed_outdoor, o.icing);
    let mut outputs = openkwl_app::Outputs::default();
    let started = Instant::now();

    loop {
        let unix = (start_unix as i64 + sim_seconds as i64 + clock_offset) as u64;
        let mut a = app.lock().map_err(|e| e.to_string())?;
        let hour = a.local_time().map_or(12.0, |t| f32::from(t.minute) / 60.0);
        let inputs = plant.step(&outputs, hour);
        outputs = a.tick_second(&inputs, clock_valid.then_some(unix));
        let synced = clock_valid.then_some(start_unix + sim_seconds / 3600 * 3600);
        a.set_connectivity(Connectivity {
            network: openkwl_api::NetworkDetails {
                link_up: true,
                dhcp: true,
                ip: Some("127.0.0.1".try_into().unwrap_or_default()),
                prefix_len: Some(8),
                ntp_server: Some("127.0.0.53".try_into().unwrap_or_default()),
                phy: "simulated".try_into().unwrap_or_default(),
                ..Default::default()
            },
            sd_card: true,
            last_time_sync_unix: synced,
            time_server: if synced.is_some() { "127.0.0.53".try_into().unwrap_or_default() } else { Default::default() },
            time_syncs: u32::from(synced.is_some()) * (1 + (sim_seconds / 3600) as u32),
            board: openkwl_api::BoardDetails {
                card_detect_present: Some(true),
                sd_power_active_low: Some(true),
                mains_pulses_per_s: Some(100),
                phy_address: Some(0),
                ntc_supply_mv: Some(3200),
                ntc_supply_used_mv: Some(3200),
                ..Default::default()
            },
            ..Default::default()
        });
        let writes = a.take_writes();
        card.persist(&a, &writes).map_err(|e| format!("sd: {e}"))?;
        let actions = a.take_actions();
        if let Some(t) = actions.set_time_unix {
            clock_offset = t as i64 - (start_unix + sim_seconds) as i64;
            clock_valid = true;
            println!("rtc: set to {t}");
        }
        if actions.mqtt_changed {
            println!("mqtt: settings changed (the simulator has no MQTT client yet)");
        }
        if actions.reboot {
            println!("reboot: reloading from the SD card");
            // What the bootloader does with an armed update: install it, rename the file.
            let settings = std::fs::read(o.sd.join("settings.ini")).unwrap_or_default();
            if let Some(image) = openkwl_update::named_image(&settings)
                && std::fs::rename(o.sd.join(image), o.sd.join("basback.bin")).is_ok()
            {
                println!("reboot: {image} \"installed\" (renamed to basback.bin)");
            }
            let mut p = platform(&o.sd);
            p.chip.reset_reason = openkwl_api::ResetReason::Software;
            let boot = card.boot(p, false).map_err(|e| e.to_string())?;
            *a = App::new(boot);
        }
        drop(a);

        sim_seconds += 1;
        if sim_seconds.is_multiple_of(u64::from(o.speed)) {
            let target = started + Duration::from_secs(sim_seconds / u64::from(o.speed));
            if let Some(wait) = target.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
    }
}
