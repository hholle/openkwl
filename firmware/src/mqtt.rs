//! MQTT client for Home Assistant: packets from `openkwl_ha::mqtt`, discovery/state/commands
//! from `openkwl-ha` via `openkwl-app` (all host-tested); this task does DNS, TCP and timing.
//!
//! Plain MQTT on the LAN (no TLS, README *Decisions*). Settings changes in the UI signal
//! [`RECONNECT`].

use core::{
    fmt::Write as _,
    str::FromStr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use defmt::{debug, info, warn};
use embassy_futures::select::{Either3, select3};
use embassy_net::{IpAddress, Ipv4Address, Stack, dns::DnsQueryType, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use embedded_io_async::Write;
use openkwl_ha::{ENTITIES, OFFLINE, ONLINE, mqtt};

use crate::{SharedApp, net};

pub static CONNECTED: AtomicBool = AtomicBool::new(false);
/// Successful connections and messages sent since boot (`GET /system/details`).
pub static CONNECTS: AtomicU32 = AtomicU32::new(0);
pub static PUBLISHED: AtomicU32 = AtomicU32::new(0);
/// Raised by the control loop when MQTT settings change.
pub static RECONNECT: Signal<CriticalSectionRawMutex, ()> = Signal::new();

const KEEP_ALIVE_S: u16 = 60;
/// How often the app is asked whether Home Assistant needs a new state
/// (`App::ha_state_due`: switching changes at once, measurements rate-limited, heartbeat).
const CHECK_EVERY: Duration = Duration::from_secs(1);
/// Keep-alive ping, well inside the broker's 1.5 × keep-alive limit.
const PING_EVERY: Duration = Duration::from_secs(30);
/// How long the broker may take to answer a PINGREQ.
const PONG_WITHIN: Duration = Duration::from_secs(15);

/// Connection settings copied out of the app, so the app lock is not held while talking
/// to the broker.
struct Settings {
    host: heapless::String<64>,
    port: u16,
    username: heapless::String<32>,
    password: heapless::String<64>,
    client_id: heapless::String<23>,
    base: heapless::String<48>,
    discovery: bool,
    prefix: heapless::String<48>,
    name: heapless::String<32>,
    model: heapless::String<24>,
    mac: heapless::String<17>,
}

async fn settings(app: &SharedApp) -> Option<Settings> {
    let a = app.lock().await;
    let c = a.config();
    let m = &c.mqtt;
    m.enabled.then(|| Settings {
        host: m.host.clone(),
        port: m.port,
        username: m.username.clone(),
        password: a.secrets().mqtt_password.clone(),
        client_id: m.client_id.clone(),
        base: m.base_topic.clone(),
        discovery: m.discovery,
        prefix: m.discovery_prefix.clone(),
        name: c.network.hostname.clone(),
        model: c.device.identity.model.clone(),
        mac: c.device.mac.clone(),
    })
}

#[embassy_executor::task]
pub async fn mqtt_task(stack: Stack<'static>, app: &'static SharedApp) -> ! {
    let mut rx = [0u8; 1024];
    let mut tx = [0u8; 2048];
    loop {
        CONNECTED.store(false, Ordering::Relaxed);
        stack.wait_config_up().await;
        let Some(s) = settings(app).await else {
            // Disabled: wait for a settings change.
            RECONNECT.wait().await;
            continue;
        };
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        socket.set_timeout(Some(Duration::from_secs(u64::from(KEEP_ALIVE_S) * 2)));
        match session(&mut socket, stack, app, &s).await {
            Ok(()) => debug!("mqtt: reconnecting with new settings"),
            Err(e) => {
                warn!("mqtt: {}", e);
                Timer::after(Duration::from_secs(15)).await;
            }
        }
        socket.abort();
        let _ = socket.flush().await;
    }
}

async fn resolve(stack: Stack<'static>, host: &str) -> Result<IpAddress, &'static str> {
    if let Ok(ip) = Ipv4Address::from_str(host) {
        return Ok(IpAddress::Ipv4(ip));
    }
    stack
        .dns_query(host, DnsQueryType::A)
        .await
        .ok()
        .and_then(|a| a.first().copied())
        .ok_or("broker not found")
}

async fn send(socket: &mut TcpSocket<'_>, bytes: &[u8]) -> Result<(), &'static str> {
    socket.write_all(bytes).await.map_err(|_| "write failed")
}

/// Every message we publish is retained. One shared await point instead of one per call
/// site keeps the session's state machine smaller.
async fn publish(socket: &mut TcpSocket<'_>, out: &mut [u8], topic: &str, payload: &[u8]) -> Result<(), &'static str> {
    let n = mqtt::publish(out, topic, payload, true).map_err(|_| "publish")?;
    send(socket, &out[..n]).await
}

/// One connection; returns `Ok` when asked to reconnect, `Err` on failure.
async fn session(
    socket: &mut TcpSocket<'_>,
    stack: Stack<'static>,
    app: &'static SharedApp,
    s: &Settings,
) -> Result<(), &'static str> {
    let mut out = [0u8; 1536];
    let mut inbuf = [0u8; 1024];
    let mut json = [0u8; openkwl_ha::STATE_JSON_MAX];

    let addr = resolve(stack, &s.host).await?;
    with_timeout(Duration::from_secs(10), socket.connect((addr, s.port)))
        .await
        .map_err(|_| "connect timeout")?
        .map_err(|_| "connect failed")?;

    let mut url: heapless::String<40> = heapless::String::new();
    if let Some(ip) = *net::ADDRESS.lock().await {
        let _ = write!(url, "http://{ip}/");
    }
    let mac: heapless::String<17> = s.mac.chars().map(|c| c.to_ascii_lowercase()).collect();
    let device = openkwl_ha::Device {
        node_id: &s.client_id,
        name: &s.name,
        sw_version: env!("CARGO_PKG_VERSION"),
        model: &s.model,
        mac: &mac,
        configuration_url: (!url.is_empty()).then_some(url.as_str()),
        base_topic: &s.base,
        discovery_prefix: &s.prefix,
    };
    let availability = device.availability_topic().map_err(|_| "topic")?;
    let state_topic = device.state_topic().map_err(|_| "topic")?;
    let ha_status = device.ha_status_topic().map_err(|_| "topic")?;

    let n = mqtt::connect(
        &mut out,
        &mqtt::Connect {
            client_id: &s.client_id,
            username: (!s.username.is_empty()).then_some(s.username.as_str()),
            password: (!s.password.is_empty()).then_some(s.password.as_bytes()),
            will: Some(mqtt::Will { topic: &availability, payload: OFFLINE.as_bytes(), retain: true }),
            keep_alive_s: KEEP_ALIVE_S,
        },
    )
    .map_err(|_| "connect packet")?;
    send(socket, &out[..n]).await?;
    let mut filled = 0;
    let code = loop {
        let r = with_timeout(Duration::from_secs(10), socket.read(&mut inbuf[filled..]))
            .await
            .map_err(|_| "no CONNACK")?
            .map_err(|_| "read")?;
        if r == 0 {
            return Err("closed");
        }
        filled += r;
        if let Ok(Some((mqtt::Incoming::ConnAck { code }, used))) = mqtt::parse(&inbuf[..filled]) {
            inbuf.copy_within(used..filled, 0);
            filled -= used;
            break code;
        }
    };
    if code != 0 {
        return Err("broker refused the connection (check user and password)");
    }
    info!("mqtt: connected");

    let publish_discovery = async |socket: &mut TcpSocket<'_>, out: &mut [u8]| -> Result<(), &'static str> {
        if !s.discovery {
            return Ok(());
        }
        let mut payload = [0u8; openkwl_ha::DISCOVERY_JSON_MAX];
        for e in ENTITIES {
            let topic = device.discovery_topic(e).map_err(|_| "topic")?;
            let body = device.discovery_payload(e, &mut payload).map_err(|_| "discovery")?;
            publish(socket, out, &topic, body).await?;
        }
        Ok(())
    };
    publish_discovery(socket, &mut out).await?;
    publish(socket, &mut out, &availability, ONLINE.as_bytes()).await?;
    let filters = device.command_filters().map_err(|_| "topic")?;
    let subs = [filters[0].as_str(), filters[1].as_str(), filters[2].as_str(), filters[3].as_str(), ha_status.as_str()];
    let n = mqtt::subscribe(&mut out, 1, &subs).map_err(|_| "subscribe")?;
    send(socket, &out[..n]).await?;
    CONNECTED.store(true, Ordering::Relaxed);
    CONNECTS.fetch_add(1, Ordering::Relaxed);

    // Publish the full state right after (re)connecting.
    let mut force = true;
    let mut last_ping = Instant::now();
    // Sent a PINGREQ and not heard back yet: a broker gone silent is noticed within seconds.
    let mut pinged: Option<Instant> = None;
    // Bytes still to drop of a packet too large for `inbuf` (a big retained message).
    let mut skip = 0usize;
    loop {
        let due = app.lock().await.ha_state_due(force);
        if let Some(state) = due {
            let body_len = state.to_json(&mut json).map(<[u8]>::len).map_err(|_| "state")?;
            PUBLISHED.fetch_add(1, Ordering::Relaxed);
            publish(socket, &mut out, &state_topic, &json[..body_len]).await?;
            force = false;
        }
        if last_ping.elapsed() >= PING_EVERY {
            send(socket, &mqtt::PINGREQ).await?;
            last_ping = Instant::now();
            pinged.get_or_insert(last_ping);
        }
        if pinged.is_some_and(|t| t.elapsed() >= PONG_WITHIN) {
            return Err("broker not answering");
        }
        match select3(socket.read(&mut inbuf[filled..]), Timer::after(CHECK_EVERY), RECONNECT.wait()).await {
            Either3::First(Ok(0)) | Either3::First(Err(_)) => return Err("connection lost"),
            Either3::First(Ok(r)) => {
                let dropped = skip.min(r);
                skip -= dropped;
                inbuf.copy_within(filled + dropped..filled + r, filled);
                filled += r - dropped;
                while let Ok(Some((packet, used))) = mqtt::parse(&inbuf[..filled]) {
                    match packet {
                        mqtt::Incoming::Publish { topic, payload, .. } if topic == ha_status.as_str() => {
                            if payload == ONLINE.as_bytes() {
                                // Home Assistant restarted: it needs the discovery messages again.
                                publish_discovery(socket, &mut out).await?;
                            }
                        }
                        // A retained command is an old one the broker replays on every
                        // connect (a stray retained "restart" would restart us each time).
                        mqtt::Incoming::Publish { retained: true, .. } => warn!("mqtt: retained command ignored"),
                        mqtt::Incoming::Publish { topic, payload, .. } => {
                            if app.lock().await.ha_command(topic, payload) {
                                // Confirm the command to HA at once.
                                force = true;
                            }
                        }
                        mqtt::Incoming::SubAck { granted: false, .. } => {
                            return Err("broker refused the subscription (check its ACL)");
                        }
                        mqtt::Incoming::PingResp => pinged = None,
                        _ => {}
                    }
                    inbuf.copy_within(used..filled, 0);
                    filled -= used;
                }
                // A broken length field: the stream cannot be followed any more. (A complete
                // packet with a broken body comes back as `Other` and is skipped above.)
                if mqtt::packet_len(&inbuf[..filled]).is_err() {
                    return Err("malformed packet");
                }
                if filled == inbuf.len() {
                    // Too large for us (no command is): drop it and carry on.
                    let total = mqtt::packet_len(&inbuf[..filled]).ok().flatten().map_or(filled, |(_, t)| t);
                    warn!("mqtt: skipped a {} byte message", total);
                    skip = total.saturating_sub(filled);
                    filled = 0;
                }
            }
            Either3::Second(()) => {}
            Either3::Third(()) => {
                // Settings changed. Entities whose topics change, or that are turned off, are
                // removed from Home Assistant (empty retained config); then "offline", since a
                // clean disconnect suppresses the last will.
                let next = settings(app).await;
                let moved = next.as_ref().is_none_or(|n| {
                    !n.discovery
                        || n.prefix != s.prefix
                        || n.base != s.base
                        || n.client_id != s.client_id
                        || n.host != s.host
                        || n.port != s.port
                });
                if s.discovery && moved {
                    for e in ENTITIES {
                        let topic = device.discovery_topic(e).map_err(|_| "topic")?;
                        publish(socket, &mut out, &topic, b"").await?;
                    }
                }
                publish(socket, &mut out, &availability, OFFLINE.as_bytes()).await?;
                let _ = send(socket, &mqtt::DISCONNECT).await;
                return Ok(());
            }
        }
    }
}
