//! Minimal HTTP/1.1 server: `/api/v1/*` goes to `openkwl-app`, everything else is served from
//! the web build directory (the firmware does the same from `/WWW` on the SD card).

use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use openkwl_app::{App, Method, Request};
use rand_core::OsRng;

const MAX_HEADER_LINES: usize = 64;
const MAX_BODY: usize = openkwl_app::http::IMPORT_BODY_MAX;

pub fn serve(listener: TcpListener, app: Arc<Mutex<App>>, www: Option<PathBuf>, sd: PathBuf) {
    for stream in listener.incoming().flatten() {
        let app = Arc::clone(&app);
        let (www, sd) = (www.clone(), sd.clone());
        thread::spawn(move || {
            if let Err(e) = handle(stream, &app, www.as_deref(), &sd)
                && e.kind() != io::ErrorKind::UnexpectedEof
            {
                eprintln!("http: {e}");
            }
        });
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

fn respond(s: &mut TcpStream, status: u16, content_type: &str, body: &[u8], extra: &str) -> io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}\r\n",
        reason(status),
        body.len()
    )?;
    s.write_all(body)
}

fn handle(mut stream: TcpStream, app: &Mutex<App>, www: Option<&Path>, sd: &Path) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/").to_owned());
    let (mut length, mut authorization, mut json, mut host) = (0usize, None::<String>, false, None::<String>);
    for _ in 0..MAX_HEADER_LINES {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "authorization" => authorization = Some(v.trim().to_owned()),
                "content-type" => json = v.trim().starts_with("application/json"),
                "host" => host = Some(v.trim().to_owned()),
                _ => {}
            }
        }
    }
    let path = target.split('?').next().unwrap_or("/").to_owned();
    if method == "PUT" && path == "/api/v1/system/update" {
        if !app.lock().expect("app lock").session_valid(authorization.as_deref()) {
            return respond(&mut stream, 401, "application/json", br#"{"error":"unauthorized","message":"log in first"}"#, "");
        }
        return match crate::update::receive(&mut reader, length, sd) {
            Ok((crc, files)) => {
                app.lock().expect("app lock").update_ready(crc, files);
                respond(&mut stream, 204, "application/json", b"", "")
            }
            Err((status, code, message)) => {
                let body = format!(r#"{{"error":"{code}","message":"{message}"}}"#);
                respond(&mut stream, status, "application/json", body.as_bytes(), "")
            }
        };
    }
    if length > MAX_BODY {
        return respond(&mut stream, 413, "application/json", br#"{"error":"payload_too_large","message":"body too large"}"#, "");
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    // Stored day records straight from the card, as the firmware (`http.rs`, `serve_days`).
    if method == "GET"
        && let Some(arg) = path.strip_prefix(openkwl_api::paths::HISTORY_DAYS)
    {
        use openkwl_app::history::{RECORD_LEN, days_requested, slot_offset, valid};
        if !app.lock().expect("app lock").session_valid(authorization.as_deref()) {
            return respond(&mut stream, 401, "application/json", br#"{"error":"unauthorized","message":"log in first"}"#, "");
        }
        let Some((first, count)) = days_requested(arg) else {
            return respond(
                &mut stream,
                400,
                "application/json",
                br#"{"error":"bad_request","message":"use YYYY-MM or YYYY-MM-DD"}"#,
                "",
            );
        };
        let mut body = Vec::new();
        for day in first..first + u16::from(count) {
            if let Some(r) = crate::storage::days_read(sd, slot_offset(day), RECORD_LEN).filter(|r| valid(r, day)) {
                body.extend_from_slice(&r);
            }
        }
        return respond(&mut stream, 200, openkwl_app::http::BINARY, &body, "");
    }
    if path.starts_with("/api/") {
        // As the firmware: the throttle locks per IPv4 client (IPv6, e.g. ::1, counts as one).
        let client = match stream.peer_addr().map(|a| a.ip()) {
            Ok(std::net::IpAddr::V4(a)) => u32::from_be_bytes(a.octets()),
            _ => 0,
        };
        let req = Request {
            method: Method::parse(method),
            path: &path,
            authorization: authorization.as_deref(),
            client,
            host: host.as_deref(),
            json,
            body: &body,
        };
        let mut out = vec![0u8; 16 * 1024];
        let r = app.lock().expect("app lock").handle(&req, &mut OsRng, &mut out);
        let extra = r.retry_after_s.map(|s| format!("Retry-After: {s}\r\n")).unwrap_or_default();
        return respond(&mut stream, r.status, r.content_type, &out[..r.len], &extra);
    }
    serve_static(&mut stream, www, &path)
}

fn serve_static(stream: &mut TcpStream, www: Option<&Path>, path: &str) -> io::Result<()> {
    let Some(root) = www else {
        return respond(stream, 404, "text/plain", b"No web UI directory given (--www web/dist).\n", "");
    };
    let rel = Path::new(path.trim_start_matches('/'));
    // Only plain path components: never serve outside the web root.
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return respond(stream, 404, "text/plain", b"not found\n", "");
    }
    // Paths without an extension are the web UI's routes (`/schedule/week`), like the firmware.
    let file = if !path.contains('.') { root.join("index.html") } else { root.join(rel) };
    match fs::read(&file) {
        Ok(bytes) => {
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
            let ct = match ext {
                "html" => "text/html; charset=utf-8",
                "js" => "text/javascript; charset=utf-8",
                "css" => "text/css; charset=utf-8",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "png" => "image/png",
                "webmanifest" => "application/manifest+json",
                "woff2" => "font/woff2",
                "txt" => "text/plain; charset=utf-8",
                _ => "application/octet-stream",
            };
            // As the unit: not to be framed by other pages (clickjacking), no type sniffing.
            respond(stream, 200, ct, &bytes, "X-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n")
        }
        Err(_) => respond(stream, 404, "text/plain", b"not found\n", ""),
    }
}
