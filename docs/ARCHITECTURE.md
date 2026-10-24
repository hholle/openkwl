# Architecture

How openKWL is put together, and why. For building and testing, see
[DEVELOPMENT.md](DEVELOPMENT.md). For the hardware layer in detail, see
[firmware/README.md](../firmware/README.md).

## Goals and constraints

| Constraint | Consequence |
|---|---|
| The **stock bootloader** stays and installs images from the SD card | The image must fit its slot: 223.5 KB of flash at `0x08008000`, CRC trailer, initial stack pointer in `0x2001xxxx`. Flash is the scarcest resource. |
| 128 KB of RAM, no heap | Only bounded types (`heapless`), fixed buffers, and a worst-case test for every buffer size |
| It controls a ventilation unit in someone's home | The control loop runs at a higher priority than everything else, and the control logic first reimplements the stock firmware's observed behaviour (same thresholds and states) before anything changes |
| Nobody can step through code inside a running unit | Everything except the drivers runs and is tested on the host, and the simulator runs the same code |

## Layers

```
            openKWL web UI (web/)             Home Assistant
                    │ HTTP /api/v1                  │ MQTT 3.1.1
┌───────────────────┼───────────────────────────────┼───────────────────┐
│ openkwl-app  (no_std)  REST handlers · control loop · persistence      │
│   ├─ openkwl-core    stock behaviour (frost, bypass, NTC, fans), modes,│
│   │                 schedule, holiday, clock/DST, SNTP, heat, filter  │
│   ├─ openkwl-api     request/response types (= api/openapi.yaml)       │
│   ├─ openkwl-auth    PBKDF2 passwords, sessions, login throttle        │
│   ├─ openkwl-config  CONFIG.INI, SECRETS.INI, AUTH.BIN, STATEA/B.BIN   │
│   ├─ openkwl-ha      MQTT topics, HA discovery, commands, MQTT codec   │
│   ├─ openkwl-bus     Modbus RTU and the KWL-KNX module protocol        │
│   └─ openkwl-update  update packages, image checks, settings.ini switch│
└───────────────────┬─────────────────────────────────┬─────────────────┘
       firmware/ (Embassy, STM32F207)           tools/openkwl-sim (host)
       drivers, SD card, network                thermal model, files, HTTP
```

**Rule:** all behaviour lives in the `no_std`, allocation-free crates and is tested on the host.
`firmware/` and `tools/openkwl-sim` are two thin platforms around `openkwl-app`. A change in
behaviour is a change in a crate, with a test.

## Crates and tools

| Path | Contents |
|---|---|
| `crates/openkwl-core` | Stock behaviour reimplemented (NTC, fan volts, tacho/stall, efficiency, bypass, frost, CWP parser) and new logic (clock/DST, schedule, holiday, modes, heat recovery, air flow, mDNS, SNTP, filter) |
| `crates/openkwl-api` | REST types and validation; a test keeps `api/openapi.yaml` in sync |
| `crates/openkwl-auth` | Password records, sessions, login throttle |
| `crates/openkwl-config` | `CONFIG.INI`/`SECRETS.INI`, `AUTH.BIN`, `STATEA.BIN`/`STATEB.BIN` records, write timing |
| `crates/openkwl-ha` | MQTT topics, discovery (29 entities), state, commands, MQTT 3.1.1 codec |
| `crates/openkwl-bus` | Modbus RTU master and the KWL-KNX module protocol (blocks byte-identical to stock) |
| `crates/openkwl-update` | Update packages (firmware + web UI): streaming reader, the bootloader's image checks, the `settings.ini` switch |
| `crates/openkwl-app` | The device application shared by firmware and simulator: control loop, API, persistence, Home Assistant, KNX |
| `tools/openkwl-sim` | Simulator: thermal model, SD card directory, HTTP |
| `tools/openkwl-import` | Reads a stock unit without logging in, and writes `HELIOS/CONFIG.INI` and the counters |
| `tools/openkwl-pack` | `verify` / `pack` firmware images for the stock bootloader, `bundle` update files |
| `web/` | openKWL UI (Vite, React 19, Mantine 9), with vitest tests; `pnpm package:sd` → `dist-sd/WWW/` |
| `firmware/` | Embassy firmware for the STM32F207 (no host tests; CI builds it); `firmware/card.sh` builds a test card |
| `vendor/serde-json-core.patch` | Upstream (downloaded by `scripts/vendor.sh`) plus a smaller float parser and writer ([vendor/README.md](../vendor/README.md)) |

Every crate and tool has host tests (`cargo test`). A few are ignored by default because they
need stock Helios files, which are not in the repository: the images
(`OPENKWL_STOCK_DIR=<folder> cargo test -p openkwl-pack -- --ignored`) and a parameter file
(`OPENKWL_CWP=<file> cargo test -p openkwl-core -- --ignored`).

`firmware/` is outside the Cargo workspace because it builds only for `thumbv7m-none-eabi`.

## The application: `openkwl-app`

`App` is one struct that holds the whole device state. It does no I/O itself; the platform
feeds it plain values and carries out what it returns:

| Call | When | Does |
|---|---|---|
| `App::new(Boot)` | At start-up | Takes what was read from the card (config, secrets, password record, counters) and the platform facts |
| `App::tick_second(&Inputs, time)` → `Outputs` | Every second, from the control loop | Takes the four temperatures and both fan speeds; runs modes, schedule, holiday, frost, bypass and the filter counter; returns the fan volts and duty and the bypass position |
| `App::handle` / `password_work` + `handle_hashed` | Per HTTP request to `/api/v1` | Routing, auth, validation, JSON in and out. Password hashing runs between the two calls, without the app lock. |
| `App::take_writes` → `Writes` | Asked by the storage task | Which files are due for saving (the [write budget](PLAN.md#31-sd-card-write-budget)) |
| `App::history_finished` | After `Writes::day` | The finished day's record and its place in `HELIOS/DAYS.BIN`; the platform writes it and serves stored days straight from the card (`/history/days/…`) |
| `App::take_actions` → `Actions` | Every second | One-shot requests: restart, set the clock, reconnect MQTT |
| `App::ha_state_due`, `App::ha_command` | Every second / per MQTT message | When to publish to Home Assistant; applies commands |
| `App::set_connectivity` | When it changes | Network, SD card, MQTT and module status, for the API and fault detection |

Because of this split, the simulator and the firmware run the same `App` and differ only in
where the values come from: the host clock, a thermal model, a directory and a TCP listener, or
the RTC, the ADC and tacho inputs, SDIO and Ethernet.

## The firmware runtime

```
 interrupt executor (UART4, high priority)      thread executor (normal)
 ┌───────────────────────────────────────┐      ┌──────────────────────────────────────┐
 │ control_task   1 Hz: App::tick_second │      │ net (embassy-net, DHCP, smoltcp)      │
 │                → PWM, bypass, LEDs    │      │ http ×3 workers   mqtt   sntp   mdns  │
 │                feeds the watchdog     │      │ storage_task (SD card: all writes)    │
 │ tacho, power-fail edge timing         │      │ bus (KNX module)  alive_task          │
 └───────────────────────────────────────┘      └──────────────────────────────────────┘
          ▲  reads net::NETWORK snapshot                    │ storage::request()
          └─────────────────────────────────────────────────┘
```

- **The control loop can't be blocked** by a slow SD card or a busy network. It never touches
  either directly: it hands writes to the storage task and reads a network snapshot.
- **The watchdog** (8 s) is fed only by the control loop, and only while the normal executor
  also makes progress (`alive_task`). Either one hanging restarts the unit.
- **Three HTTP workers**, each with its own request buffer. Large bodies are streamed: the
  config import through a shared 12 KB buffer, update packages straight onto the card.

Details, including the start-up LED states and the memory budgets, are in
[firmware/README.md](../firmware/README.md).

## Data on the SD card

openKWL keeps its own files apart from the stock files, which it never changes (one agreed
exception: an armed [update](UPDATING.md) rewrites the first line of `settings.ini`, after a
backup).

| Path | Written | Contents |
|---|---|---|
| `HELIOS/CONFIG.INI` | When a setting changes (10 s debounce) | All settings, plain text, editable by hand |
| `HELIOS/CONFIG.BAK` | Before every `CONFIG.INI` save, and at boot after a hand edit | The same settings: whichever survives a power cut is used |
| `HELIOS/SECRETS.INI`, `SECRETS.BAK` | When the MQTT password changes (copy first) | MQTT password |
| `HELIOS/AUTH.BIN`, `AUTH.BAK` | When the password changes (copy first) | Salted PBKDF2 hash |
| `HELIOS/STATEA.BIN`, `STATEB.BIN` | Every 6 h, on mode changes, on power failure | Counters and mode, A/B in place, so a power cut never loses both |
| `WWW/`, `WWW2/` | Only by an update | The web UI, gzipped, 8.3 names |

The full card layout is in [INSTALL.md](INSTALL.md#what-is-on-the-card). The write budget is
about 10 writes a day ([PLAN.md §3.1](PLAN.md#31-sd-card-write-budget)).

## Web UI

- React 19 + Mantine 9, built by Vite into `index.html`, `app.js` and `app.css`.
- `scripts/package-sd.mjs` gzips them and maps long names to 8.3 names.
- The firmware serves the files straight from the card (`WWW/`), not from flash. The UI costs no
  firmware space, and an update can replace it.
- The UI polls `/api/v1/status` every second while visible. Home Assistant gets changes by MQTT
  within about a second.
- The API types are generated from `api/openapi.yaml` (`pnpm gen:api`).

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Language | Rust, `no_std`, Embassy async | Memory safety without a heap; host tests for all logic; one codebase for firmware and simulator |
| Boot | Keep the stock bootloader; install from the SD card (`FwBasis=` in `settings.ini`) | Going back uses the same path as installing (tried on one unit); no SWD probe needed |
| Control logic | Reimplements the stock firmware's observed behaviour (same thresholds and states), quirks included, in new code; no Helios code | Known, field-tested behaviour first; improvements go into new modules, after asking |
| Transport security | Plain HTTP on the LAN; TLS on a reverse proxy | TLS doesn't fit: about 35 KB of RAM and more flash than is left ([SECURITY.md](../SECURITY.md)) |
| Auth | One admin password, PBKDF2-HMAC-SHA256 with salt, per-browser tokens from the hardware RNG, expiry, throttle | A home device with one owner; safe against password guessing on the LAN |
| HTTP, MQTT | Own small HTTP/1.1 server and MQTT 3.1.1 client | The existing crates need `std` or more flash than the slot has |
| JSON | `serde-json-core` (patched at build time, smaller float code); hand-written writer for MQTT documents | Flash: about 15 KB saved |
| Stock files | Own files in `HELIOS/`, `WWW/`, `WWW2/`; stock files are never changed | Rollback to the stock firmware keeps working with the same card |
| Updates | Package checked completely before the bootloader is armed; alternating image names and web folders | A failed or damaged upload can never break the unit or an armed update |
| BEC panel, heaters | Not supported, not planned | The web UI and Home Assistant replace the panel; a heater needs its own safety concept (independent cut-off, bench tests) |

## Where to read the code first

1. `crates/openkwl-app/src/lib.rs`: the `App` struct and `tick_second`.
2. `crates/openkwl-app/src/http.rs`: the API routes.
3. `crates/openkwl-core/src/mode.rs` and `schedule.rs`: what "Auto" means.
4. `firmware/src/control.rs`: how the firmware drives the app every second.
5. `tools/openkwl-sim/src/main.rs`: the same, on your computer.
