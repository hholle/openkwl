//! `/api/v1` request handling. The platform's HTTP server parses the request line, the
//! `Authorization` header and the body, calls [`App::handle`] and sends the returned status,
//! content type and the bytes written to `out`.

use core::fmt::Write as _;

use openkwl_api::json::{FromJson, Reader, Seen};
use openkwl_api::{
    ApiError, ControlRequest, ErrorCode, FieldError, LoginRequest, LoginResponse, Mode, MqttConfigUpdate, PasswordChangeRequest,
    SetTimeRequest, SetupRequest, SetupStatus, SystemInfo, Validate, auth::Password, time::DateTime,
};
use openkwl_auth::{DEFAULT_ITERATIONS, PasswordRecord, Token};
use openkwl_config::ini;
use openkwl_core::{mode::ProgramKind, schedule::PROFILES};
use rand_core::{CryptoRng, RngCore};
use serde::Serialize;

use crate::{App, SETUP_WINDOW_S, buf::SliceWriter};

pub const JSON: &str = "application/json";
pub const TEXT: &str = "text/plain; charset=utf-8";
/// `PUT /config/import` takes a whole `CONFIG.INI`, larger than JSON bodies.
pub const IMPORT_BODY_MAX: usize = openkwl_config::CONFIG_MAX_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Other,
}

impl Method {
    pub fn parse(s: &str) -> Self {
        match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            _ => Method::Other,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub method: Method,
    /// Path without query string, e.g. `/api/v1/status`.
    pub path: &'a str,
    /// Value of the `Authorization` header.
    pub authorization: Option<&'a str>,
    /// The client's IPv4 address (`u32`, big-endian octets; 0 if unknown): the login throttle
    /// locks per client, so someone guessing on the network can't lock the owner out.
    pub client: u32,
    /// The `Host` header. First setup only answers to the unit's own name or an IP address,
    /// against DNS rebinding (a foreign page reaching the unit under a name it controls).
    pub host: Option<&'a str>,
    /// `Content-Type: application/json`. Setup and login, the only requests without a session,
    /// accept nothing else: a foreign web page can send a form or plain text to the unit without
    /// asking, but JSON only after a CORS preflight the unit never allows (CSRF).
    pub json: bool,
    pub body: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    /// Bytes of `out` to send.
    pub len: usize,
    /// For 429: value of the `Retry-After` header.
    pub retry_after_s: Option<u64>,
}

impl Response {
    const fn empty(status: u16) -> Self {
        Self { status, content_type: JSON, len: 0, retry_after_s: None }
    }
}

type Result<T> = core::result::Result<T, Response>;

// `json` and `parse` stay out of line: inlined, their error paths (and `parse`'s 2 KB scratch
// buffer) were copied into every route, about 2.2 KB of flash and a much larger `route` frame.
#[inline(never)]
fn json<T: Serialize>(status: u16, v: &T, out: &mut [u8]) -> Response {
    match serde_json_core::to_slice(v, out) {
        Ok(len) => Response { status, content_type: JSON, len, retry_after_s: None },
        Err(_) => error(ErrorCode::Internal, "response too large", out),
    }
}

fn error(code: ErrorCode, message: &str, out: &mut [u8]) -> Response {
    let len = serde_json_core::to_slice(&ApiError::new(code, message), out).unwrap_or(0);
    Response { status: code.http_status(), content_type: JSON, len, retry_after_s: None }
}

fn invalid(e: FieldError, out: &mut [u8]) -> Response {
    let len = serde_json_core::to_slice(&ApiError::invalid(e), out).unwrap_or(0);
    Response { status: 422, content_type: JSON, len, retry_after_s: None }
}

#[inline(never)]
fn parse<T: FromJson>(body: &[u8], out: &mut [u8]) -> Result<T> {
    if body.len() > openkwl_api::REQUEST_BODY_MAX {
        return Err(error(ErrorCode::PayloadTooLarge, "body too large", out));
    }
    // Not serde: one shared reader instead of a generated parser per type (`openkwl_api::json`).
    openkwl_api::json::from_slice(body).map_err(|_| error(ErrorCode::BadRequest, "malformed JSON", out))
}

fn parse_valid<T: FromJson + Validate>(body: &[u8], out: &mut [u8]) -> Result<T> {
    let v: T = parse(body, out)?;
    v.validate().map_err(|e| invalid(e, out))?;
    Ok(v)
}

/// The password hashing a request needs. PBKDF2 takes about a second on the unit, so the
/// firmware gets this from [`App::password_work`], runs it with the app unlocked and passes
/// the result to [`App::handle_hashed`]; the control loop keeps its 1 s rhythm meanwhile.
#[derive(Debug, Clone, Default)]
pub struct PasswordWork {
    check: Option<(PasswordRecord, Password)>,
    create: Option<Password>,
}

/// Results of [`PasswordWork::run`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Hashed {
    checked: Option<(PasswordRecord, bool)>,
    created: Option<PasswordRecord>,
}

impl PasswordWork {
    pub fn is_empty(&self) -> bool {
        self.check.is_none() && self.create.is_none()
    }

    /// Does the hashing. `salt` (random) is used for a new password.
    pub fn run(&self, salt: [u8; openkwl_auth::SALT_LEN]) -> Hashed {
        Hashed {
            checked: self.check.as_ref().map(|(r, pw)| (*r, r.verify(pw.as_bytes()))),
            created: self.create.as_ref().map(|pw| PasswordRecord::with_salt(pw.as_bytes(), salt, DEFAULT_ITERATIONS)),
        }
    }
}

impl Hashed {
    /// The precomputed check if it was made against `record`, else hashes now.
    fn verify(&self, record: &PasswordRecord, password: &str) -> bool {
        match self.checked {
            Some((r, ok)) if r == *record => ok,
            _ => record.verify(password.as_bytes()),
        }
    }

    fn create<R: RngCore + CryptoRng>(&self, password: &str, rng: &mut R) -> PasswordRecord {
        self.created.unwrap_or_else(|| PasswordRecord::new(password.as_bytes(), DEFAULT_ITERATIONS, rng))
    }
}

/// The three password requests (setup, login, password change) in one JSON shape: the
/// firmware then carries one JSON parser for them instead of three (about 1.7 KB of flash).
/// Each endpoint still requires its own fields; on the wire nothing changes.
#[derive(Default)]
struct AuthBody {
    password: Option<Password>,
    remember: bool,
    current_password: Option<Password>,
    new_password: Option<Password>,
}

impl FromJson for AuthBody {
    fn from_json(r: &mut Reader<'_>) -> core::result::Result<Self, openkwl_api::json::Error> {
        let mut b = AuthBody::default();
        let mut seen = Seen::default();
        let text = |r: &mut Reader<'_>| if r.null() { Ok(None) } else { r.string().map(Some) };
        r.object(&mut |r, k| {
            match k {
                b"password" => (seen.mark(0)?, b.password = text(r)?),
                b"remember" => (seen.mark(1)?, b.remember = r.bool()?),
                b"current_password" => (seen.mark(2)?, b.current_password = text(r)?),
                b"new_password" => (seen.mark(3)?, b.new_password = text(r)?),
                _ => ((), r.skip()?),
            };
            Ok(())
        })?;
        Ok(b)
    }
}

trait FromAuth: Sized + Validate {
    fn from_auth(b: AuthBody) -> Option<Self>;
}

impl FromAuth for SetupRequest {
    fn from_auth(b: AuthBody) -> Option<Self> {
        Some(Self { password: b.password? })
    }
}

impl FromAuth for LoginRequest {
    fn from_auth(b: AuthBody) -> Option<Self> {
        Some(Self { password: b.password?, remember: b.remember })
    }
}

impl FromAuth for PasswordChangeRequest {
    fn from_auth(b: AuthBody) -> Option<Self> {
        Some(Self { current_password: b.current_password?, new_password: b.new_password? })
    }
}

/// [`parse_valid`] for the password requests.
fn parse_auth<T: FromAuth>(req: &Request<'_>, out: &mut [u8]) -> Result<T> {
    if !req.json {
        return Err(error(ErrorCode::BadRequest, "JSON only", out));
    }
    let b: AuthBody = parse(req.body, out)?;
    let r = T::from_auth(b).ok_or_else(|| error(ErrorCode::BadRequest, "malformed JSON", out))?;
    r.validate().map_err(|e| invalid(e, out))?;
    Ok(r)
}

/// Whether `host` (the `Host` header) names the unit itself: an IP address, `localhost`, or
/// its name alone or with a home-network suffix. DNS rebinding needs a name the attacker
/// controls (`openkwl.evil.example` is not accepted). No header (HTTP/1.0 tools) is fine:
/// browsers always send one.
pub(crate) fn local_host(host: Option<&str>, hostname: &str) -> bool {
    // Byte loops on purpose: `str::split`/`rsplit_once` cost about 1 KB of flash here.
    const SUFFIXES: [&[u8]; 8] = [b"", b".", b".local", b".lan", b".home", b".home.arpa", b".fritz.box", b".internal"];
    let Some(h) = host else { return true };
    let h = h.as_bytes();
    if h.first() == Some(&b'[') {
        return true; // an IPv6 address
    }
    let end = match h.iter().rposition(|&c| c == b':') {
        Some(i) if h[i + 1..].iter().all(u8::is_ascii_digit) => i,
        _ => h.len(),
    };
    let name = &h[..end];
    // Digits and three dots: an IPv4 address, which a browser never looks up in DNS.
    let ipv4 = !name.is_empty()
        && name.iter().all(|&c| c.is_ascii_digit() || c == b'.')
        && name.iter().filter(|&&c| c == b'.').count() == 3;
    let hn = hostname.as_bytes();
    let ours = name.len() >= hn.len()
        && name[..hn.len()].eq_ignore_ascii_case(hn)
        && SUFFIXES.iter().any(|s| name[hn.len()..].eq_ignore_ascii_case(s));
    ipv4 || ours || name.eq_ignore_ascii_case(b"localhost")
}

/// 429 from the login throttle, with `Retry-After`.
fn throttled(wait: u64, out: &mut [u8]) -> Response {
    let len = serde_json_core::to_slice(&ApiError::throttled(wait), out).unwrap_or(0);
    Response { status: 429, content_type: JSON, len, retry_after_s: Some(wait) }
}

fn login_response(token: &Token, remember: bool, out: &mut [u8]) -> Response {
    let hex = token.to_hex();
    let token = heapless::String::try_from(core::str::from_utf8(&hex).unwrap_or_default()).unwrap_or_default();
    let (idle, max) =
        if remember { (crate::REMEMBER_IDLE_S, crate::REMEMBER_MAX_S) } else { (crate::SESSION_IDLE_S, crate::SESSION_MAX_S) };
    json(200, &LoginResponse { token, idle_timeout_s: idle as u32, expires_in_s: max as u32 }, out)
}

impl App {
    /// Whether `authorization` carries a valid session, for requests the platform handles
    /// itself (the firmware update upload).
    pub fn session_valid(&mut self, authorization: Option<&str>) -> bool {
        let now = self.uptime_s;
        authorization
            .and_then(Token::from_bearer)
            .is_some_and(|t| self.sessions.validate(&t, now) || self.remembered.validate(&t, now))
    }

    /// Notes a stored update in the log; it installs at the next restart.
    pub fn update_ready(&mut self, image_crc: u16, web_files: u16) {
        self.log(format_args!("firmware update {image_crc:04x} ready ({web_files} web files), installs at the next restart"));
    }

    fn setup_open(&self) -> bool {
        self.auth.is_none() && self.uptime_s < SETUP_WINDOW_S
    }

    /// Handles one API request; writes the body into `out` (at least 2 KB; 12 KB for
    /// `GET /config/export`). Hashes passwords inline; see [`App::password_work`].
    pub fn handle<R: RngCore + CryptoRng>(&mut self, req: &Request<'_>, rng: &mut R, out: &mut [u8]) -> Response {
        self.handle_hashed(req, rng, out, Hashed::default())
    }

    /// What [`App::handle_hashed`] will need hashed for `req`: nothing for most requests;
    /// only requests that would get that far (setup open, login not throttled, a valid
    /// session for a password change) get work, so nobody can make the unit hash at will.
    pub fn password_work(&mut self, req: &Request<'_>) -> PasswordWork {
        let mut work = PasswordWork::default();
        let Some(path) = req.path.strip_prefix(openkwl_api::PREFIX) else { return work };
        let mut err = [0u8; 96];
        let now = self.uptime_s;
        match (req.method, path) {
            (Method::Post, "/setup") if self.setup_open() => {
                work.create = parse_auth::<SetupRequest>(req, &mut err).ok().map(|r| r.password);
            }
            (Method::Post, "/auth/login") if self.throttle.check(req.client, now).is_ok() => {
                if let (Some(record), Ok(r)) = (self.auth, parse_auth::<LoginRequest>(req, &mut err)) {
                    work.check = Some((record, r.password));
                }
            }
            (Method::Post, "/auth/password") if self.throttle.check(req.client, now).is_ok() => {
                let token = req.authorization.and_then(Token::from_bearer);
                let session = token.is_some_and(|t| self.sessions.validate(&t, now) || self.remembered.validate(&t, now));
                if let (true, Some(record), Ok(r)) = (session, self.auth, parse_auth::<PasswordChangeRequest>(req, &mut err)) {
                    work.check = Some((record, r.current_password));
                    work.create = Some(r.new_password);
                }
            }
            _ => {}
        }
        work
    }

    /// [`App::handle`] with password hashes computed beforehand ([`PasswordWork::run`]).
    pub fn handle_hashed<R: RngCore + CryptoRng>(
        &mut self,
        req: &Request<'_>,
        rng: &mut R,
        out: &mut [u8],
        hashed: Hashed,
    ) -> Response {
        let r = match self.route(req, rng, out, &hashed) {
            Ok(r) | Err(r) => r,
        };
        self.stats.http_requests = self.stats.http_requests.saturating_add(1);
        if r.status >= 400 {
            self.stats.http_errors = self.stats.http_errors.saturating_add(1);
        }
        r
    }

    fn route<R: RngCore + CryptoRng>(
        &mut self,
        req: &Request<'_>,
        rng: &mut R,
        out: &mut [u8],
        hashed: &Hashed,
    ) -> Result<Response> {
        let Some(path) = req.path.strip_prefix(openkwl_api::PREFIX) else {
            return Ok(error(ErrorCode::NotFound, "not found", out));
        };
        let now = self.uptime_s;
        use Method::*;

        // Setup and login work without a session.
        match (req.method, path) {
            (Get, "/setup") => {
                let open = self.auth.is_none();
                let window = open.then(|| SETUP_WINDOW_S.saturating_sub(now) as u32);
                return Ok(json(200, &SetupStatus { setup_required: open, window_remaining_s: window }, out));
            }
            (Post, "/setup") => {
                if !self.setup_open() {
                    return Ok(error(ErrorCode::SetupRequired, "setup is not open", out));
                }
                if !local_host(req.host, &self.config.network.hostname) {
                    return Ok(error(ErrorCode::BadRequest, "open setup by the unit's address", out));
                }
                let r: SetupRequest = parse_auth(req, out)?;
                self.auth = Some(hashed.create(&r.password, rng));
                self.auth_dirty = true;
                self.log(format_args!("admin password set"));
                let token = self.sessions.create(now, rng);
                return Ok(login_response(&token, false, out));
            }
            _ => {}
        }
        let Some(record) = self.auth else {
            return Ok(error(ErrorCode::SetupRequired, "set a password first (/setup)", out));
        };
        if (req.method, path) == (Post, "/auth/login") {
            if let Err(wait) = self.throttle.check(req.client, now) {
                return Ok(throttled(wait, out));
            }
            let r: LoginRequest = parse_auth(req, out)?;
            if !hashed.verify(&record, &r.password) {
                self.throttle.record_failure(req.client, now);
                self.stats.failed_logins = self.stats.failed_logins.saturating_add(1);
                self.log(format_args!("failed login"));
                return Ok(error(ErrorCode::Unauthorized, "wrong password", out));
            }
            self.throttle.record_success(req.client);
            let token = if r.remember { self.remembered.create(now, rng) } else { self.sessions.create(now, rng) };
            self.log(format_args!("login{}", if r.remember { " (stay logged in)" } else { "" }));
            return Ok(login_response(&token, r.remember, out));
        }

        // Everything else needs a valid session.
        let token = req.authorization.and_then(Token::from_bearer);
        if !token.is_some_and(|t| self.sessions.validate(&t, now) || self.remembered.validate(&t, now)) {
            return Ok(error(ErrorCode::Unauthorized, "log in first", out));
        }
        let token = token.unwrap_or(Token([0; openkwl_auth::TOKEN_LEN]));

        Ok(match (req.method, path) {
            (Post, "/auth/logout") => {
                self.sessions.revoke(&token);
                self.remembered.revoke(&token);
                Response::empty(204)
            }
            (Post, "/auth/password") => {
                // The same throttle as login: a session (up to 90 days with "stay logged in")
                // must not be a way to guess the password.
                if let Err(wait) = self.throttle.check(req.client, now) {
                    return Ok(throttled(wait, out));
                }
                let r: PasswordChangeRequest = parse_auth(req, out)?;
                if !hashed.verify(&record, &r.current_password) {
                    self.throttle.record_failure(req.client, now);
                    self.stats.failed_logins = self.stats.failed_logins.saturating_add(1);
                    self.log(format_args!("failed login"));
                    return Ok(error(ErrorCode::Unauthorized, "wrong password", out));
                }
                self.throttle.record_success(req.client);
                self.auth = Some(hashed.create(&r.new_password, rng));
                self.auth_dirty = true;
                self.sessions.revoke_all();
                self.remembered.revoke_all();
                self.log(format_args!("password changed"));
                Response::empty(204)
            }
            (Get, "/status") => json(200, &self.status(), out),
            (Get, "/control") => json(200, &self.control(), out),
            (Patch, "/control") => {
                let r: ControlRequest = parse_valid(req.body, out)?;
                if let Err(e) = self.apply_control(&r) {
                    return Ok(invalid(e, out));
                }
                json(200, &self.control(), out)
            }
            (Get, "/system") => json(200, &self.system_info(), out),
            (Get, "/system/details") => {
                let d = self.system_details();
                json(200, &d, out)
            }
            (Post, "/system/time") => {
                let r: SetTimeRequest = parse_valid(req.body, out)?;
                self.unix_s = Some(r.unix_s);
                self.actions.set_time_unix = Some(r.unix_s);
                self.stats.time_set_manually = true;
                self.log(format_args!("clock set manually"));
                Response::empty(204)
            }
            // The platform's HTTP server streams the upload itself (it does not fit a request
            // buffer); reaching this means the platform has no update support.
            (Put, "/system/update") => error(ErrorCode::Unavailable, "firmware update is not available in this build", out),
            (Post, "/system/reboot") => {
                self.actions.reboot = true;
                self.log(format_args!("restart requested"));
                Response::empty(202)
            }
            (Post, "/filter/reset") => {
                self.filter_reset();
                self.log(format_args!("filter interval restarted"));
                Response::empty(204)
            }
            (Get, "/log") => Response { status: 200, content_type: TEXT, len: self.log.copy_to(out), retry_after_s: None },
            (Get, "/config/export") => {
                let mut w = SliceWriter::new(out);
                if ini::write_config(&self.config, &mut w).is_err() {
                    return Ok(error(ErrorCode::Internal, "buffer too small", out));
                }
                Response { status: 200, content_type: TEXT, len: w.len(), retry_after_s: None }
            }
            (Put, "/config/import") => self.import(req.body, out),
            (m, p) if p.starts_with("/config/") => self.config_route(m, &p["/config/".len()..], req.body, out)?,
            _ => error(ErrorCode::NotFound, "not found", out),
        })
    }

    /// Applies a mode/level change. While a holiday runs, everything but `end_program` is
    /// refused: a level would only change the manual level behind the holiday and stay after
    /// it (fans off for good after "off"). The holiday ends via `/config/holiday`.
    pub(crate) fn apply_control(&mut self, r: &ControlRequest) -> core::result::Result<(), FieldError> {
        if self.holiday.is_some() && !r.end_program {
            let field = if r.mode.is_some() { "mode" } else { "level" };
            return Err(FieldError { field, reason: "a holiday program is running; end it first" });
        }
        let p = self.config.programs;
        match r.mode {
            _ if r.end_program => self.mode.end_program(),
            Some(Mode::Auto) => self.mode.set_auto(),
            Some(Mode::Manual) => {
                self.mode.set_manual();
                if let Some(l) = r.level {
                    self.mode.set_level(l);
                }
            }
            Some(kind @ (Mode::Party | Mode::Quiet)) => {
                let (k, level, minutes) = match kind {
                    Mode::Party => (ProgramKind::Party, p.party_level, p.party_min),
                    _ => (ProgramKind::Quiet, p.quiet_level, p.quiet_min),
                };
                let minutes = r.duration_min.unwrap_or(minutes);
                self.mode.start_program(k, r.level.unwrap_or(level), u32::from(minutes) * 60);
            }
            Some(Mode::Holiday) => {}
            None => {
                if let Some(l) = r.level {
                    self.mode.set_level(l);
                }
            }
        }
        let c = self.control();
        self.log(format_args!("control: {:?} level {}", c.mode, c.level));
        Ok(())
    }

    fn mac(&self) -> heapless::String<17> {
        let mut mac = heapless::String::new();
        for ch in self.config.device.mac.chars() {
            let _ = mac.push(ch.to_ascii_lowercase());
        }
        mac
    }

    fn local(&self, unix_s: Option<u64>) -> Option<DateTime> {
        unix_s.map(|t| DateTime::from(self.config.time.zone().to_local(t)))
    }

    fn system_info(&self) -> SystemInfo {
        let c = &self.connectivity;
        let p = &self.platform;
        SystemInfo {
            firmware_version: p.firmware.version.clone(),
            build: p.firmware.build.clone(),
            mcu: p.chip.mcu.clone(),
            mac: self.mac(),
            ip: c.network.ip.clone(),
            uptime_s: self.uptime_s.min(u64::from(u32::MAX)) as u32,
            reset_reason: p.chip.reset_reason,
            sd_card: c.sd_card,
            mqtt_connected: c.mqtt_connected,
            last_time_sync: self.local(c.last_time_sync_unix),
            device: self.config.device.identity.clone(),
        }
    }

    fn system_details(&mut self) -> openkwl_api::SystemDetails {
        use openkwl_api::system::*;
        let now = self.uptime_s;
        let sessions = self.sessions.active(now) + self.remembered.active(now);
        let c = &self.connectivity;
        let p = &self.platform;
        let mut network = c.network.clone();
        if network.hostname.is_empty() {
            network.hostname = self.config.network.hostname.clone();
        }
        if network.mac.is_empty() {
            network.mac = self.mac();
        }
        let source = if c.last_time_sync_unix.is_some() {
            TimeSource::Sntp
        } else if self.stats.time_set_manually {
            TimeSource::Manual
        } else if self.unix_s.is_some() {
            TimeSource::Rtc
        } else {
            TimeSource::None
        };
        SystemDetails {
            firmware: FirmwareDetails { uptime_s: self.uptime_s.min(u64::from(u32::MAX)) as u32, ..p.firmware.clone() },
            chip: p.chip.clone(),
            memory: MemoryDetails { stack_peak_bytes: c.stack_peak_bytes, ..p.memory.clone() },
            network,
            time: TimeDetails {
                source,
                server: c.time_server.clone(),
                last_sync: self.local(c.last_time_sync_unix),
                last_offset_ms: c.time_offset_ms,
                syncs: c.time_syncs,
                rtc_clock: p.rtc_clock.clone(),
            },
            mqtt: MqttDetails {
                enabled: self.config.mqtt.enabled,
                connected: c.mqtt_connected,
                broker: self.config.mqtt.host.clone(),
                connects: c.mqtt_connects,
                published: c.mqtt_published,
                received: self.stats.mqtt_received,
            },
            storage: StorageDetails {
                present: c.sd_card,
                capacity_mb: c.sd_capacity_mb,
                writes: self.stats.writes,
                last_state_save: self.local(self.stats.last_state_save_unix),
                config_valid: !self.config_invalid,
            },
            stock: p.stock.clone(),
            board: c.board.clone(),
            modules: c.modules.clone(),
            counters: Counters {
                http_requests: self.stats.http_requests,
                http_errors: self.stats.http_errors,
                failed_logins: self.stats.failed_logins,
                sessions: sessions.min(255) as u8,
            },
        }
    }

    fn import(&mut self, body: &[u8], out: &mut [u8]) -> Response {
        if body.len() > IMPORT_BODY_MAX {
            return error(ErrorCode::PayloadTooLarge, "file too large", out);
        }
        let Ok(text) = core::str::from_utf8(body) else {
            return error(ErrorCode::BadRequest, "not UTF-8 text", out);
        };
        match ini::read_config(text, self.config.clone()) {
            Ok((mut next, _)) => {
                // The device identity is read-only.
                next.device = self.config.device.clone();
                // A file pointing at another broker does not get the stored password.
                if (next.mqtt.host != self.config.mqtt.host || next.mqtt.port != self.config.mqtt.port)
                    && !self.secrets.mqtt_password.is_empty()
                {
                    self.secrets.mqtt_password.clear();
                    self.secrets_dirty = true;
                    self.log(format_args!("MQTT broker changed by import: password removed"));
                }
                next.mqtt.password_set = !self.secrets.mqtt_password.is_empty();
                self.config = next;
                self.config_changed();
                self.actions.mqtt_changed = true;
                self.log(format_args!("settings file imported"));
                Response::empty(204)
            }
            Err(e) => {
                let mut msg: heapless::String<96> = heapless::String::new();
                let _ = match e.kind {
                    ini::ErrorKind::Invalid(section, f) => {
                        write!(msg, "[{section}] {}: {}", f.field, f.reason)
                    }
                    ini::ErrorKind::Syntax => {
                        write!(msg, "line {}: not a key = value line", e.line)
                    }
                    ini::ErrorKind::BadValue => write!(msg, "line {}: bad value", e.line),
                };
                let len = serde_json_core::to_slice(&ApiError::new(ErrorCode::Invalid, &msg), out).unwrap_or(0);
                Response { status: 422, content_type: JSON, len, retry_after_s: None }
            }
        }
    }

    fn config_route(&mut self, m: Method, key: &str, body: &[u8], out: &mut [u8]) -> Result<Response> {
        macro_rules! section {
            ($field:ident, $ty:ty) => {{
                if m == Method::Put {
                    let v: $ty = parse_valid(body, out)?;
                    self.config.$field = v;
                    self.config_changed();
                    self.log(format_args!(concat!("saved ", stringify!($field))));
                } else if m != Method::Get {
                    return Ok(error(ErrorCode::MethodNotAllowed, "method not allowed", out));
                }
                json(200, &self.config.$field, out)
            }};
        }
        Ok(match key {
            "bypass" => section!(bypass, openkwl_api::BypassConfig),
            "frost" => section!(frost, openkwl_api::FrostConfig),
            "fans" => section!(fans, openkwl_api::FanConfig),
            "programs" => section!(programs, openkwl_api::ProgramConfig),
            "holiday" => section!(holiday, openkwl_api::HolidayConfig),
            "time" => section!(time, openkwl_api::TimeConfig),
            "filter" => section!(filter, openkwl_api::FilterConfig),
            "network" => section!(network, openkwl_api::NetworkConfig),
            "schedule" => section!(schedule, openkwl_api::ScheduleConfig),
            "mqtt" => {
                if m == Method::Put {
                    let u: MqttConfigUpdate = parse_valid(body, out)?;
                    // The stored password never follows the settings to another broker: whoever
                    // can change the broker must also know (and type) the password.
                    let moved = u.host != self.config.mqtt.host || u.port != self.config.mqtt.port;
                    if moved && u.password.is_none() && !self.secrets.mqtt_password.is_empty() {
                        let e = FieldError { field: "password", reason: "enter the password again for a new broker" };
                        return Ok(invalid(e, out));
                    }
                    if let Some(p) = &u.password {
                        self.secrets.mqtt_password = p.clone();
                        self.secrets_dirty = true;
                    }
                    self.config.mqtt = u.applied(!self.secrets.mqtt_password.is_empty());
                    self.config_changed();
                    self.actions.mqtt_changed = true;
                } else if m != Method::Get {
                    return Ok(error(ErrorCode::MethodNotAllowed, "method not allowed", out));
                }
                let mut view = self.config.mqtt.clone();
                view.password_set = !self.secrets.mqtt_password.is_empty();
                json(200, &view, out)
            }
            other => {
                let n = other.strip_prefix("schedule/").and_then(|n| n.parse::<usize>().ok());
                let Some(n) = n.filter(|n| (1..=PROFILES).contains(n)) else {
                    return Ok(error(ErrorCode::NotFound, "not found", out));
                };
                if m == Method::Put {
                    let p: openkwl_api::ScheduleProfile = parse_valid(body, out)?;
                    self.config.profiles[n - 1] = p;
                    self.config_changed();
                    self.log(format_args!("saved weekly program {n}"));
                } else if m != Method::Get {
                    return Ok(error(ErrorCode::MethodNotAllowed, "method not allowed", out));
                }
                json(200, &self.config.profiles[n - 1], out)
            }
        })
    }
}
