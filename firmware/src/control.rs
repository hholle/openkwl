//! The 1 Hz loop: RTC → `App::tick_second` → outputs, persistence, platform actions,
//! watchdog. Runs in the main task.

use core::sync::atomic::{AtomicU32, Ordering};

use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_stm32::{
    rtc::{DateTime, DayOfWeek, Rtc, RtcTimeProvider},
    wdg::IndependentWatchdog,
};
use embassy_time::{Duration, Ticker, Timer, with_timeout};
use openkwl_app::Connectivity;
use openkwl_core::clock::{self, Date};

use crate::{SharedApp, io, mqtt, net, sntp, storage, sysinfo};

fn rtc_unix(t: &RtcTimeProvider) -> Option<u64> {
    let now = t.now().ok()?;
    let date = Date::new(now.year(), now.month(), now.day())?;
    let unix =
        u64::from(date.days()) * 86_400 + u64::from(now.hour()) * 3600 + u64::from(now.minute()) * 60 + u64::from(now.second());
    (unix >= clock::MIN_VALID_UNIX_S).then_some(unix)
}

fn set_rtc(rtc: &mut Rtc, unix: u64) {
    let date = Date::from_days((unix / 86_400) as u32);
    let s = (unix % 86_400) as u32;
    let dow = [
        DayOfWeek::Monday,
        DayOfWeek::Tuesday,
        DayOfWeek::Wednesday,
        DayOfWeek::Thursday,
        DayOfWeek::Friday,
        DayOfWeek::Saturday,
        DayOfWeek::Sunday,
    ][date.weekday()];
    match DateTime::from(date.year, date.month, date.day, dow, (s / 3600) as u8, (s / 60 % 60) as u8, (s % 60) as u8, 0) {
        Ok(dt) => {
            if rtc.set_datetime(dt).is_err() {
                warn!("rtc: set failed");
            }
        }
        Err(_) => warn!("rtc: invalid time"),
    }
}

/// Mains is going away (stock: outputs off at once, then save within 650 ms, then wait for
/// the reset). Outputs off first, then counters and pending settings go to the storage task;
/// if the supply holds up after all (a short dip), restart cleanly like the stock firmware.
async fn power_fail(app: &'static SharedApp, drive: &mut io::Drive, leds: &mut io::Leds) -> ! {
    drive.all_off();
    leds.off();
    warn!("power failing, saving");
    {
        let mut a = app.lock().await;
        a.power_failing();
        storage::request(a.take_writes());
        // A signal left over from an earlier save must not end the wait below.
        storage::SAVED.reset();
    }
    let _ = with_timeout(Duration::from_millis(600), storage::SAVED.wait()).await;
    Timer::after(Duration::from_secs(1)).await;
    cortex_m::peripheral::SCB::sys_reset();
}

/// Counted up once a second by [`alive_task`] in the normal executor (network, HTTP, MQTT,
/// SD card).
static ALIVE: AtomicU32 = AtomicU32::new(0);
/// The control loop only feeds the watchdog while the normal executor also makes progress, so
/// a hang there (Ethernet DMA, SD card, a stuck future) restarts the unit too. Generous: SD
/// writes and password hashing block that executor for a second or two.
const ALIVE_LIMIT_S: u32 = 30;

#[embassy_executor::task]
pub async fn alive_task() -> ! {
    loop {
        ALIVE.fetch_add(1, Ordering::Relaxed);
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// The 1 Hz loop. Runs in the high-priority interrupt executor (`main.rs`): it must not touch
/// the SD card (storage task) or the IP stack (`net::NETWORK` is a snapshot) so that neither a
/// slow card nor network traffic can delay the fans or the watchdog.
#[embassy_executor::task]
pub async fn control_task(
    app: &'static SharedApp,
    mut drive: io::Drive,
    mut leds: io::Leds,
    mut rtc: Rtc,
    rtc_time: RtcTimeProvider,
    mut watchdog: IndependentWatchdog<'static, embassy_stm32::peripherals::IWDG>,
) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    let mut last_sync: Option<u64> = None;
    let mut last_offset_ms: Option<i32> = None;
    let mut stack_peak = 0;
    let mut seconds: u32 = 0;
    let (mut last_alive, mut stalled_s) = (0u32, 0u32);
    loop {
        if let Either::Second(()) = select(ticker.next(), io::POWER_FAIL.wait()).await {
            power_fail(app, &mut drive, &mut leds).await;
        }
        let alive = ALIVE.load(Ordering::Relaxed);
        if alive != last_alive {
            (last_alive, stalled_s) = (alive, 0);
        } else {
            stalled_s += 1;
        }
        if stalled_s < ALIVE_LIMIT_S {
            watchdog.pet();
        } else if stalled_s == ALIVE_LIMIT_S {
            warn!("normal executor stalled for {} s, letting the watchdog restart", ALIVE_LIMIT_S);
        }
        let network = net::NETWORK.lock(|n| n.borrow().clone());
        leds.tick(network.link_up);

        if let Some(t) = sntp::SYNCED.try_take() {
            last_offset_ms =
                rtc_unix(&rtc_time).map(|r| ((t as i64 - r as i64) * 1000).clamp(i32::MIN.into(), i32::MAX.into()) as i32);
            set_rtc(&mut rtc, t);
            last_sync = Some(t);
        }
        if seconds.is_multiple_of(10) {
            stack_peak = sysinfo::stack_peak_bytes();
        }
        seconds = seconds.wrapping_add(1);
        let unix = rtc_unix(&rtc_time);
        if let Some(t) = unix {
            storage::NOW_UNIX.store(t.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
        }
        let inputs = io::inputs();

        let mut a = app.lock().await;
        let dhcp = a.config().network.dhcp;
        a.set_connectivity(Connectivity {
            network: openkwl_api::NetworkDetails { dhcp, ..network },
            sd_card: storage::PRESENT.load(Ordering::Relaxed),
            sd_capacity_mb: storage::capacity_mb(),
            mqtt_connected: mqtt::CONNECTED.load(Ordering::Relaxed),
            mqtt_connects: mqtt::CONNECTS.load(Ordering::Relaxed),
            mqtt_published: mqtt::PUBLISHED.load(Ordering::Relaxed),
            last_time_sync_unix: last_sync,
            time_server: sntp::SERVER.lock(|s| s.borrow().clone()),
            time_offset_ms: last_offset_ms,
            time_syncs: sntp::SYNCS.load(Ordering::Relaxed),
            stack_peak_bytes: Some(stack_peak),
            board: sysinfo::board(),
            modules: crate::bus::details(),
        });
        let outputs = a.tick_second(&inputs, unix);
        let bypass = a.config().bypass;
        drive.apply(&outputs, bypass.actuator, bypass.run_time_s);

        storage::request(a.take_writes());
        let actions = a.take_actions();
        drop(a);

        if actions.mqtt_changed {
            mqtt::RECONNECT.signal(());
        }
        if let Some(t) = actions.set_time_unix {
            set_rtc(&mut rtc, t);
        }
        if actions.reboot {
            info!("restart requested");
            drive.all_off();
            // Settings and counters first (take_writes forced them), and let the HTTP response
            // go out, then reset. The request above went out without an await since, so
            // resetting here only drops signals from earlier saves.
            storage::SAVED.reset();
            let _ = with_timeout(Duration::from_secs(3), storage::SAVED.wait()).await;
            Timer::after(Duration::from_millis(500)).await;
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}
