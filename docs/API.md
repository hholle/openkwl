# REST API

The web UI uses this API, and you can too: for scripts, dashboards or your own integration.
The contract is [`api/openapi.yaml`](../api/openapi.yaml) (OpenAPI 3.1). Load it into any
OpenAPI viewer or client generator. This page is a hands-on guide.

> [!TIP]
> For Home Assistant, use [MQTT](HOME_ASSISTANT.md) rather than the REST API: it pushes
> changes instead of polling.

## Basics

- **Base URL:** `http://<unit>/api/v1`, for example `http://openkwl.local/api/v1`.
- **JSON** in and out, UTF-8. Units are °C, V and seconds unless a field name says otherwise
  (`_min`, `_pct`, `_m3h`, …). Missing readings are `null`.
- **Authentication:** `Authorization: Bearer <token>` on every endpoint except
  `POST /auth/login` and `/setup`.
- **Methods:** `GET` reads; `PUT` replaces a whole settings object; `PATCH /control` changes
  only the fields you send; `POST` triggers an action.
- **Limits:** request bodies up to 2048 bytes (`PUT /config/import`: 12 KB;
  `PUT /system/update`: 2 MB). Three requests are handled at a time.
- **Plain HTTP.** Tokens are readable on the wire, so use it only on a trusted network, or
  behind a TLS reverse proxy ([SECURITY.md](../SECURITY.md)).
- **Try it without hardware:** `./dev.sh` starts the simulator with the same API on
  `http://localhost:8080/api/v1`.

## Log in

```sh
UNIT=http://openkwl.local
TOKEN=$(curl -s -X POST $UNIT/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"password":"your password"}' | jq -r .token)
```

```json
{ "token": "3f9c…64 hex digits…", "idle_timeout_s": 1800, "expires_in_s": 43200 }
```

- A session ends after 30 minutes without use, and after 12 hours at most.
- With `"remember": true` it lasts 30 days without use, and 90 days at most.
- Logout (`POST /auth/logout`), a password change or a restart of the unit end it early.
- **Throttle:** after 3 wrong passwords from one IP address, logins from that address are locked
  for a doubling time, up to 15 minutes; other devices are not affected. Above 30 failures in
  15 minutes from all addresses together, logins pause for everyone. The answer is `429` with
  `retry_after_s`.

**First setup:** on a new installation every endpoint answers `403 setup_required` until a
password is set. `POST /setup` with `{"password": "…"}` (10–64 characters) sets it. It is
accepted only while no password exists, within 30 minutes after power-on, and only when the
unit is addressed by its IP address or its own name (against DNS rebinding).

## Examples

Read the live values:

```sh
curl -s $UNIT/api/v1/status -H "Authorization: Bearer $TOKEN" | jq '{mode: .control.mode, level: .control.level, t: .temperatures}'
```

```json
{
  "mode": "auto",
  "level": 1,
  "t": { "outdoor": 13.7, "supply": 20.6, "extract": 22.1, "exhaust": 15.8 }
}
```

Switch to level 3 (manual; during Party or Quiet this changes the program's level, and while a
holiday runs it is refused with 422 until the holiday is ended in `/config/holiday`):

```sh
curl -s -X PATCH $UNIT/api/v1/control -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"level": 3}'
```

Start Party for 30 minutes at level 4, then end it early:

```sh
curl -s -X PATCH $UNIT/api/v1/control -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"mode": "party", "level": 4, "duration_min": 30}'
curl -s -X PATCH $UNIT/api/v1/control -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"end_program": true}'
```

Back to the weekly program:

```sh
curl -s -X PATCH $UNIT/api/v1/control -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"mode": "auto"}'
```

Back up all settings, and restore them later:

```sh
curl -s $UNIT/api/v1/config/export -H "Authorization: Bearer $TOKEN" -o CONFIG.INI
curl -s -X PUT $UNIT/api/v1/config/import -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: text/plain' --data-binary @CONFIG.INI
```

The export holds no passwords. The import checks the whole file first and applies nothing if
any line is wrong.

To change a setting, read the object, change it, and write it back whole:

```sh
curl -s $UNIT/api/v1/config/filter -H "Authorization: Bearer $TOKEN"     # {"interval_months":3}
curl -s -X PUT $UNIT/api/v1/config/filter -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"interval_months": 6}'
```

## Endpoints

| Method | Path | What it does |
|---|---|---|
| `GET` / `POST` | `/setup` | Whether first setup is open / set the first password |
| `POST` | `/auth/login` | Password → session token |
| `POST` | `/auth/logout` | End this session |
| `POST` | `/auth/password` | Change the password (ends every session) |
| `GET` | `/status` | Live readings: mode, level, temperatures, fans, bypass, frost, heat recovery, filter, faults, today's schedule |
| `GET` / `PATCH` | `/control` | Mode and level; start or end Party/Quiet |
| `GET` / `PUT` | `/config/schedule` | Weekly program: active profile, Back to Auto, fallback level |
| `GET` / `PUT` | `/config/schedule/{profile}` | One of the 5 profiles (1–3 stock Standard, 4–5 user-defined) |
| `GET` / `PUT` | `/config/holiday` | Holiday dates and how the fans run meanwhile |
| `GET` / `PUT` | `/config/programs` | Party and Quiet defaults (level, duration) |
| `GET` / `PUT` | `/config/bypass` | Summer cooling: target, minimum outdoor temperature, season, actuator |
| `GET` / `PUT` | `/config/frost` | Frost protection thresholds (stock defaults) |
| `GET` / `PUT` | `/config/fans` | Fan voltages per level and air flow calibration |
| `GET` / `PUT` | `/config/filter` | Filter-change interval |
| `POST` | `/filter/reset` | Restart the filter interval |
| `GET` / `PUT` | `/config/network` | DHCP or static address, host name |
| `GET` / `PUT` | `/config/mqtt` | Home Assistant / MQTT connection |
| `GET` / `PUT` | `/config/time` | SNTP server, UTC offset, EU summer time |
| `POST` | `/system/time` | Set the clock (when SNTP is off) |
| `GET` | `/config/export` | Download `CONFIG.INI` (no secrets) |
| `PUT` | `/config/import` | Upload and apply a `CONFIG.INI` |
| `GET` | `/system` | Firmware version, MAC and IP address, uptime, last reset, SD card and MQTT state |
| `GET` | `/system/details` | Everything the firmware knows about itself (*Technical details*) |
| `GET` | `/log` | Recent log lines (RAM, plain text) |
| `GET` | `/history/today` | Today so far as a day record (binary, see below) |
| `GET` | `/history/days/YYYY-MM-DD` | One stored day record; `/history/days/YYYY-MM` every stored day of the month, concatenated (empty if none) |
| `PUT` | `/system/update` | Upload an update package ([UPDATING.md](UPDATING.md#the-api)) |
| `POST` | `/system/reboot` | Restart after the answer is sent |

The request and response schemas, with every field, range and default, are in
[`api/openapi.yaml`](../api/openapi.yaml).

**Day records** (`/history/…`) are binary, 1216 bytes each, little-endian: a 64-byte header
(`"KL"`, version, date as days since 1970-01-01, CRC-32, seconds recorded, seconds per level,
bypass and frost seconds, heat and cooling recovered in Wh, mean fan speeds) and 96
quarter-hour points (four air temperatures in 0.1 °C, recovered heat power in W, level, bypass
and frost flags). The exact layout is in `crates/openkwl-app/src/history.rs` and in the
`DayRecord` schema of `api/openapi.yaml`. They are binary to keep the firmware small; the web
UI decodes them and computes the statistics. For long-term history, Home Assistant records
every value openKWL publishes.

## Errors

Errors have a status code and a JSON body:

```json
{ "error": "invalid", "message": "set via /config/holiday", "field": "mode" }
```

| Status | `error` | When |
|---|---|---|
| 400 | `bad_request` | Not valid JSON, or the wrong shape |
| 401 | `unauthorized` | No token, or an expired token, or the wrong password |
| 403 | `setup_required` | No password set yet: call `POST /setup` |
| 404 | `not_found` | Unknown path |
| 405 | `method_not_allowed` | Known path, wrong method |
| 409 | `conflict` | Another update upload is running |
| 413 | `payload_too_large` | Body over the limit |
| 422 | `invalid` | Well-formed but not allowed; `field` names the field |
| 429 | `too_many_requests` | Login throttle; wait `retry_after_s` |
| 500 | `internal` | A bug: please [report it](../../../issues/new?template=bug_report.yml) |
| 503 | `unavailable` | SD card missing, or a feature this build doesn't have |

## Generating a client

```sh
npx openapi-typescript api/openapi.yaml -o schema.d.ts    # TypeScript types, as the web UI does
```

Any OpenAPI 3.1 generator works the same way (Python, Go, …).

## Stability

The API is versioned in the path (`/api/v1`). Until the first release it may still change;
changes are listed in the [CHANGELOG](../CHANGELOG.md). After that, `v1` only gains fields and
endpoints; anything incompatible gets a new version.
