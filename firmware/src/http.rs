//! HTTP/1.1 server on port 80: `/api/v1/*` goes to `openkwl-app`, everything else is served
//! from `WWW/` on the SD card (gzipped, fonts as they are) under its 8.3 name (`/` → `INDEX.HTM`, `/app.js` →
//! `APP.JS`). Paths without a file extension are the web UI's routes (`/schedule/week`) and
//! get `INDEX.HTM`. The same split as `tools/openkwl-sim`; `HELIOS/` is never served.

use defmt::warn;
use embassy_net::{IpAddress, Stack, tcp::TcpSocket};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use embedded_io_async::Write;
use openkwl_app::{Hashed, Method, Request};

use crate::{
    SharedApp, SharedRng,
    storage::{BIG, Card, WwwError},
};

/// Concurrent connections. smoltcp has no listen backlog: a connection that arrives while
/// every worker is busy is refused, and a page load opens several at once.
pub const WORKERS: usize = 3;
const HEADER_MAX: usize = 1024;
/// JSON request bodies (`openkwl_api::REQUEST_BODY_MAX`).
const BODY_MAX: usize = openkwl_api::REQUEST_BODY_MAX;
/// Largest JSON answer (a schedule profile or the system details, 1.9 KB); also the size of
/// each piece of a web file.
const OUT_MAX: usize = 2048;
/// Bytes in flight: with only one segment the browser's delayed ACK (up to 200 ms) paces a
/// 224 KB `APP.JS`; four segments keep it streaming.
const TX_MAX: usize = 4096;
/// Answers and bodies bigger than `OUT_MAX` (`CONFIG.INI` up to 12 KB, the 4 KB RAM log, the
/// system details up to 2.4 KB)
/// use the storage's shared buffer instead of 12 KB per worker.
const LARGE_PATHS: [&str; 4] = ["/api/v1/config/import", "/api/v1/config/export", "/api/v1/log", "/api/v1/system/details"];
/// A browser's spare (preconnected) connection that sends nothing frees its worker after this.
const FIRST_BYTE_S: u64 = 3;

#[embassy_executor::task(pool_size = WORKERS)]
pub async fn server_task(
    id: usize,
    stack: Stack<'static>,
    app: &'static SharedApp,
    rng: &'static SharedRng,
    card: &'static Card,
) -> ! {
    let mut rx = [0u8; 1536];
    let mut tx = [0u8; TX_MAX];
    let mut req = [0u8; HEADER_MAX + BODY_MAX];
    let mut out = [0u8; OUT_MAX];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        if socket.accept(80).await.is_err() {
            continue;
        }
        socket.set_timeout(Some(Duration::from_secs(FIRST_BYTE_S)));
        if let Err(e) = handle(id, &mut socket, &mut req, &mut out, app, rng, card).await {
            warn!("http[{}]: {}", id, e);
        }
        // FIN first, then wait (bounded) until the client has everything: dropping the socket
        // earlier would cut the connection off with a reset.
        socket.close();
        let _ = with_timeout(Duration::from_secs(2), socket.flush()).await;
    }
}

/// A request's header and body must arrive within this, and a file must be sent within
/// `SEND_S`: each read and write has its own 10 s timeout, but without a deadline a client
/// trickling a byte every few seconds would hold one of the three workers indefinitely.
const REQUEST_S: u64 = 20;
const SEND_S: u64 = 120;

/// A header value kept as its position in the request buffer (no copy).
fn text(buf: &[u8], at: Option<(usize, usize)>) -> Option<&str> {
    at.and_then(|(a, b)| core::str::from_utf8(&buf[a..b]).ok())
}

fn too_late(deadline: Instant) -> Result<(), &'static str> {
    if Instant::now() > deadline { Err("too slow") } else { Ok(()) }
}

/// Reads the header block; returns (header end, Content-Length, bytes read so far).
async fn read_head(socket: &mut TcpSocket<'_>, buf: &mut [u8], deadline: Instant) -> Result<(usize, usize, usize), &'static str> {
    let mut n = 0;
    let header_end = loop {
        if n == buf.len() {
            return Err("request too large");
        }
        too_late(deadline)?;
        let r = socket.read(&mut buf[n..]).await.map_err(|_| "read")?;
        if r == 0 {
            return Err("closed");
        }
        n += r;
        if let Some(p) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if n >= HEADER_MAX {
            return Err("header too large");
        }
    };
    let head = core::str::from_utf8(&buf[..header_end]).map_err(|_| "header not UTF-8")?;
    let length = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    Ok((header_end, length, n))
}

/// Reads the rest of a `length`-byte body into `body`, of which `have` bytes are there.
async fn read_body(socket: &mut TcpSocket<'_>, body: &mut [u8], mut have: usize, deadline: Instant) -> Result<(), &'static str> {
    while have < body.len() {
        too_late(deadline)?;
        let r = socket.read(&mut body[have..]).await.map_err(|_| "read")?;
        if r == 0 {
            return Err("closed");
        }
        have += r;
    }
    Ok(())
}

async fn respond(socket: &mut TcpSocket<'_>, status: u16, content_type: &str, extra: &str, body: &[u8]) -> Result<(), &'static str> {
    let mut head: heapless::String<256> = heapless::String::new();
    let _ = core::fmt::write(
        &mut head,
        format_args!(
            "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
            if status < 400 { "OK" } else { "Error" },
            body.len()
        ),
    );
    socket.write_all(head.as_bytes()).await.map_err(|_| "write")?;
    socket.write_all(body).await.map_err(|_| "write")
}

async fn handle(
    id: usize,
    socket: &mut TcpSocket<'_>,
    buf: &mut [u8],
    out: &mut [u8],
    app: &'static SharedApp,
    rng: &'static SharedRng,
    card: &'static Card,
) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(REQUEST_S);
    let (header_end, length, n) = read_head(socket, buf, deadline).await?;
    // The request has started: give slow links the usual time from here on.
    socket.set_timeout(Some(Duration::from_secs(10)));
    // Copy what the router needs out of the header, so the body can be read into the buffer.
    let (method, path, authorization, versioned, json, host) = {
        let head = core::str::from_utf8(&buf[..header_end]).map_err(|_| "header")?;
        let mut lines = head.lines();
        let mut first = lines.next().unwrap_or_default().split_whitespace();
        let method = Method::parse(first.next().unwrap_or(""));
        let target = first.next().unwrap_or("/");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let path: heapless::String<128> = path.try_into().map_err(|_| "path too long")?;
        // `/app.js?v=<content hash>` from index.html: a new build is a new URL.
        let versioned = query.starts_with("v=");
        let mut authorization: Option<heapless::String<128>> = None;
        // Setup and login accept only JSON (CSRF; `openkwl_app::http::Request::json`), and
        // setup only under the unit's own name (DNS rebinding; `Request::host`).
        let mut json = false;
        let mut host: Option<(usize, usize)> = None;
        for (k, v) in lines.filter_map(|l| l.split_once(':')) {
            let (k, v) = (k.trim(), v.trim());
            if k.eq_ignore_ascii_case("authorization") {
                authorization = v.try_into().ok();
            } else if k.eq_ignore_ascii_case("content-type") {
                json = v.starts_with("application/json");
            } else if k.eq_ignore_ascii_case("host") {
                // Where the value is in `buf`: the header stays there (the body is read after it).
                let at = v.as_ptr() as usize - head.as_ptr() as usize;
                host = Some((at, at + v.len()));
            }
        }
        (method, path, authorization, versioned, json, host)
    };

    // An update is streamed onto the card, not read into a buffer (`update.rs`).
    #[cfg(feature = "update")]
    if method == Method::Put && path == "/api/v1/system/update" {
        if !app.lock().await.session_valid(authorization.as_deref()) {
            return respond(socket, 401, "application/json", "", br#"{"error":"unauthorized","message":"log in first"}"#).await;
        }
        let have = n - header_end;
        buf.copy_within(header_end..n, 0);
        // 204 when stored and armed; the details are in the unit's log.
        let (status, code, message) = match crate::update::receive(socket, length, have, buf, app, card).await {
            Ok(()) => return respond(socket, 204, "application/json", "Cache-Control: no-store\r\n", b"").await,
            Err(e) => e,
        };
        let mut body: heapless::String<128> = heapless::String::new();
        for part in [r#"{"error":""#, code, r#"","message":""#, message, r#""}"#] {
            let _ = body.push_str(part);
        }
        return respond(socket, status, "application/json", "Cache-Control: no-store\r\n", body.as_bytes()).await;
    }
    // Stored day records come straight from the card (`openkwl_app::history`).
    if method == Method::Get
        && let Some(arg) = path.strip_prefix(openkwl_api::paths::HISTORY_DAYS)
    {
        if !app.lock().await.session_valid(authorization.as_deref()) {
            return respond(socket, 401, "application/json", "", br#"{"error":"unauthorized","message":"log in first"}"#).await;
        }
        return serve_days(socket, arg, out, card).await;
    }
    if path.starts_with("/api/") {
        let too_large = br#"{"error":"payload_too_large","message":"body too large"}"#;
        let large = LARGE_PATHS.contains(&path.as_str());
        // All of them need a session: check it before taking the shared buffer, so a client
        // without one can't hold it (and with it every settings save) with a slow upload.
        if large && !app.lock().await.session_valid(authorization.as_deref()) {
            return respond(socket, 401, "application/json", "", br#"{"error":"unauthorized","message":"log in first"}"#).await;
        }
        let mut shared = if large { Some(BIG.lock().await) } else { None };
        let have = n - header_end;
        // An import's body and an export's answer use the shared buffer.
        let (body, answer): (&[u8], &mut [u8]) = match shared.as_deref_mut() {
            Some(big) if method == Method::Put => {
                if length > big.len() {
                    return respond(socket, 413, "application/json", "", too_large).await;
                }
                big[..have].copy_from_slice(&buf[header_end..n]);
                read_body(socket, &mut big[..length], have, deadline).await?;
                (&big[..length], out)
            }
            other => {
                // Not `header_end + length`: a huge Content-Length would wrap around.
                if length > buf.len() - header_end {
                    return respond(socket, 413, "application/json", "", too_large).await;
                }
                read_body(socket, &mut buf[header_end..header_end + length], have, deadline).await?;
                let body = &buf[header_end..header_end + length];
                match other {
                    Some(big) => (body, &mut big[..]),
                    None => (body, out),
                }
            }
        };
        // The login throttle locks per client address (IPv4 only on this unit).
        let client = socket.remote_endpoint().map_or(0, |ep| {
            let IpAddress::Ipv4(a) = ep.addr;
            u32::from_be_bytes(a.octets())
        });
        let req =
            Request { method, path: &path, authorization: authorization.as_deref(), client, host: text(buf, host), json, body };
        // Password hashing (about a second) runs with nothing locked, so the control loop
        // and other requests carry on meanwhile.
        let work = app.lock().await.password_work(&req);
        let hashed = if work.is_empty() {
            Hashed::default()
        } else {
            let mut salt = [0u8; openkwl_auth::SALT_LEN];
            rng.lock().await.fill_bytes(&mut salt);
            work.run(salt)
        };
        let r = {
            let mut a = app.lock().await;
            let mut rng = rng.lock().await;
            a.handle_hashed(&req, &mut *rng, answer, hashed)
        };
        let mut extra: heapless::String<48> = heapless::String::new();
        if let Some(s) = r.retry_after_s {
            let _ = core::fmt::write(&mut extra, format_args!("Retry-After: {s}\r\n"));
        }
        let _ = extra.push_str("Cache-Control: no-store\r\n");
        return respond(socket, r.status, r.content_type, &extra, &answer[..r.len]).await;
    }
    serve_www(id, socket, &path, versioned, out, card).await
}

/// One stored day (`YYYY-MM-DD`) or every stored day of a month (`YYYY-MM`), oldest first.
async fn serve_days(socket: &mut TcpSocket<'_>, arg: &str, out: &mut [u8], card: &'static Card) -> Result<(), &'static str> {
    use openkwl_app::history::{RECORD_LEN, days_requested, slot_offset, valid};
    let Some((first, count)) = days_requested(arg) else {
        return respond(socket, 400, "application/json", "", br#"{"error":"bad_request","message":"use YYYY-MM or YYYY-MM-DD"}"#)
            .await;
    };
    let record = &mut out[..RECORD_LEN];
    let read = async |day: u16, record: &mut [u8]| {
        card.lock().await.as_deref_mut().is_some_and(|s| s.days_read(slot_offset(day), record)) && valid(record, day)
    };
    // The length is not known before the card was read: the body ends with the connection.
    // An empty body means no record (one day, or none in the month).
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|_| "write")?;
    for day in first..first + u16::from(count) {
        if read(day, record).await {
            socket.write_all(record).await.map_err(|_| "write")?;
        }
    }
    Ok(())
}

/// Long names the browser asks for (favicons, fonts) → their 8.3 names on the card;
/// keep in sync with `web/scripts/package-sd.mjs`.
const ALIASES: [(&str, &str); 7] = [
    ("/apple-touch-icon.png", "APPLETCH.PNG"),
    ("/icon-192.png", "ICON192.PNG"),
    ("/icon-512.png", "ICON512.PNG"),
    ("/icon-maskable-512.png", "ICONM512.PNG"),
    ("/fonts/Geist-Variable.woff2", "GEISTVAR.WF2"),
    ("/fonts/GeistMono-Regular.woff2", "GEISTMNO.WF2"),
    ("/fonts/OFL.txt", "OFL.TXT"),
];

/// `/` and the UI's routes (no `.` in the path, e.g. `/settings/clock`) → `INDEX.HTM`,
/// `/app.js` → `APP.JS`; only single 8.3 names, no directories.
fn www_name(path: &str) -> Option<heapless::String<12>> {
    if let Some((_, name)) = ALIASES.iter().find(|(p, _)| *p == path) {
        return (*name).try_into().ok();
    }
    if path == "/site.webmanifest" {
        return "MANIFEST.WEB".try_into().ok();
    }
    let name = if !path.contains('.') { "index.html" } else { path.strip_prefix('/')? };
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = if ext.eq_ignore_ascii_case("html") { "htm" } else { ext };
    let valid =
        |s: &str, max| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid(stem, 8) || !valid(ext, 3) {
        return None;
    }
    let mut out = heapless::String::new();
    for c in stem.chars().chain(core::iter::once('.')).chain(ext.chars()) {
        out.push(c.to_ascii_uppercase()).ok()?;
    }
    Some(out)
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e) {
        Some("HTM") => "text/html; charset=utf-8",
        Some("JS") => "text/javascript; charset=utf-8",
        Some("CSS") => "text/css; charset=utf-8",
        Some("SVG") => "image/svg+xml",
        Some("ICO") => "image/x-icon",
        Some("PNG") => "image/png",
        Some("WEB") => "application/manifest+json",
        Some("WF2") => "font/woff2",
        Some("TXT") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn serve_www(
    id: usize,
    socket: &mut TcpSocket<'_>,
    path: &str,
    versioned: bool,
    chunk: &mut [u8],
    card: &'static Card,
) -> Result<(), &'static str> {
    let not_found = br#"not found"#;
    let Some(name) = www_name(path) else {
        return respond(socket, 404, "text/plain", "", not_found).await;
    };
    // The file stays open while it streams; another worker sending the same file is waited
    // for (the FAT library allows one handle per file), at most a few seconds.
    let mut len = None;
    for _ in 0..60 {
        let opened = card.lock().await.as_deref_mut().map(|s| s.www_open(id, &name));
        match opened {
            Some(Ok(n)) => len = Some(n),
            Some(Err(WwwError::Busy)) => {
                Timer::after_millis(50).await;
                continue;
            }
            _ => {}
        }
        break;
    }
    let Some(len) = len else {
        return respond(socket, 404, "text/plain", "", not_found).await;
    };
    let result = stream(id, socket, &name, len, versioned, chunk, card).await;
    if let Some(s) = card.lock().await.as_deref_mut() {
        s.www_close(id);
    }
    result
}

/// Sends the header and the open web file of worker `id`.
async fn stream(
    id: usize,
    socket: &mut TcpSocket<'_>,
    name: &str,
    len: u32,
    versioned: bool,
    chunk: &mut [u8],
    card: &'static Card,
) -> Result<(), &'static str> {
    // Fonts are stored as they are (WOFF2 is compressed already) and never change. Versioned
    // URLs (app.js, app.css) change with every build. index.html, which names the versions,
    // is checked on every load; icons and the manifest may be an hour old.
    let font = name.ends_with(".WF2");
    let cache = if name == "INDEX.HTM" {
        "no-cache"
    } else if font || versioned {
        "max-age=31536000, immutable"
    } else {
        "max-age=3600"
    };
    let encoding = if font { "" } else { "Content-Encoding: gzip\r\n" };
    let mut head: heapless::String<320> = heapless::String::new();
    let _ = core::fmt::write(
        &mut head,
        format_args!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\n{encoding}Content-Length: {len}\r\nCache-Control: {cache}\r\nX-Frame-Options: DENY\r\nConnection: close\r\n\r\n",
            content_type(name)
        ),
    );
    socket.write_all(head.as_bytes()).await.map_err(|_| "write")?;
    let mut sent = 0u32;
    let deadline = Instant::now() + Duration::from_secs(SEND_S);
    while sent < len {
        too_late(deadline)?;
        let n = card.lock().await.as_deref_mut().and_then(|s| s.www_read(id, chunk)).unwrap_or(0);
        if n == 0 {
            return Err("card read");
        }
        socket.write_all(&chunk[..n]).await.map_err(|_| "write")?;
        sent += n as u32;
    }
    Ok(())
}
