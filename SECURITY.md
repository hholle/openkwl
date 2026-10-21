# Security

openKWL controls a ventilation unit in someone's home. Please report security problems
privately, not in a public issue.

## Reporting a vulnerability

Use the repository's private vulnerability reporting (on GitHub: *Security › Report a
vulnerability*). Include the firmware version (*Settings › Technical details*), what an attacker
needs (network access, a login, physical access) and how to reproduce it. This is a hobby
project, so answers may take a while; fixes go into the next release, and you are credited
unless you prefer not to be.

Only the latest release is supported.

## Security model

openKWL is made for a **trusted home network**. Know these limits before you install it:

- **Plain HTTP.** The unit has no TLS: the password and the session token cross the network in
  clear text. Don't expose the unit to the internet or to untrusted networks (guest Wi-Fi, a
  shared building network). For access from outside, use a VPN, or a reverse proxy with TLS
  (Caddy, Home Assistant) in front of it.
- **MQTT without TLS.** The MQTT password and every command travel in clear text. Anyone who can
  publish to the broker's command topics can control the unit, so protect the broker with its
  own users and access rules.
- **One admin password.** Stored as a salted PBKDF2-HMAC-SHA256 hash (`HELIOS/AUTH.BIN`); logins
  are throttled per device (after 3 wrong passwords a doubling lockout up to 15 minutes for
  that IP address only, so someone guessing can't lock you out; above 30 failures in 15 minutes
  from all addresses together, logins pause for everyone); sessions are random tokens from the
  hardware RNG and expire. Every `/api/v1` endpoint except first setup and login needs a session.
- **Updates are checked, not signed.** The web UI's firmware update refuses damaged or wrong
  files (checksums, image checks), but anyone who is logged in can install firmware they built
  themselves. Protect the password accordingly ([docs/UPDATING.md](docs/UPDATING.md)).
- **Physical access wins.** Whoever can open the unit can read the SD card (settings, the MQTT
  password in `HELIOS/SECRETS.INI` and `SECRETS.BAK`), reset the web password with a `RESETPW` file, or install
  other firmware. The stock bootloader does not check signatures.
- **First setup is open.** Until a password is set, anyone on the network can set it, for
  30 minutes after power-on. Set it right after installing.
- **The password file must be readable.** It is saved twice (`AUTH.BAK` first, then
  `AUTH.BIN`), so a power cut while saving keeps one copy. If the SD card fails, or both are
  empty or damaged, the unit treats the password as unknown: logging in fails, and setup stays closed
  (it does not open to anyone on the network). An empty `RESETPW` file on the card opens setup
  again.
- **Other web pages can't use your browser against the unit.** Setup and login accept only JSON,
  which a foreign page can't send without the browser asking first (CORS), and every other
  request needs the session token, which other pages can't read. The web UI can't be embedded
  in another page (`X-Frame-Options: DENY`), which prevents clickjacking. The unit does not
  check the `Host` header for normal use (so reverse proxies work), but first setup only answers
  to an IP address or the unit's own name (`openkwl`, `openkwl.local`, `.lan`, `.fritz.box`, …),
  so a DNS-rebinding page can't set the password.

## In scope

The firmware, the web UI, the REST API, the MQTT integration and the tools in this repository.
Out of scope: the stock Helios firmware and bootloader, and weaknesses that need physical access
to the unit (see above).
