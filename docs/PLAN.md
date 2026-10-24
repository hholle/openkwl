# openKWL: open work and design notes

What is done is described by the code and the READMEs. This file keeps what is still open, and
the design rules the code refers to.

**Status:** openKWL first booted on one KWL EC 300 W R on 2026-10-17 and has run there for a
few days, with no frost season yet. The control logic reimplements the behaviour observed in
the stock 2.24 firmware (same thresholds and states) in new code; the image for going back is
2.27, whose control functions appeared unchanged in that analysis. No Helios code or binaries
are in the repository.

## 1. Open work, in order

1. **Finish the first test on the unit** (`docs/INSTALL.md`) and fix what it shows. Section 2
   lists what is still unconfirmed.
2. **KNX module (KWL-KNX)**: implemented (section 4); found on a unit (2026-10-24, firmware 0.16,
   level and Auto from KNX work); the remaining blocks still to watch.
3. **Firmware update from the web UI**: implemented (section 5); works on a unit (2026-10-24).
   Still to try there: going back to the original firmware with a stock-only `.okwl`.
4. Optional, later: MQTT over TLS (`embedded-tls`, about 35 KB of RAM), only if a broker outside
   the LAN is needed.
5. **History and statistics**: implemented (2026-10-24, `openkwl_app::history`, the *History*
   page): live view from `/status`, the last 24 hours and daily records on the SD card (§3.1),
   statistics computed in the browser. To confirm on the unit: the first midnight's write.
   Flash: under 1 KB left in the normal build; the next feature needs a saving first (likely
   a hand-written JSON writer for `/system/details`, whose serde code is 3.6 KB).

### Security hardening, still open

From the security review (2026-10-21); fixed then: the Content-Length crash, setup and login
from foreign pages (JSON only), clickjacking, an unreadable or empty `AUTH.BIN` opening setup,
an empty or unreadable `CONFIG.INI` overwriting its backup, the update reader's entry counter,
MQTT text that could add lines to `CONFIG.INI`, the password change without throttle, and the
shared buffer held without a session; afterwards (with 2.2 KB freed by keeping the JSON helpers
out of line): deadlines per request (20 s header and body, 120 s per file) against slow clients,
a second upload refused while an update waits for its restart, and power-cut safe settings
files (the backup is written first, then the file), the login throttle per IP address (with a
ceiling over all), and first setup only under the unit's own name or IP (DNS rebinding).
Nothing open; PBKDF2 stays at 10,000 iterations (about 1 s per login on the unit; the hash is
only on the SD card).

### Not planned

- **KWL-BEC wall panels** (module bus 0x19, 0x21–0x29): the panel uses the same block protocol as
  the KWL-KNX, but needs more blocks for its display and menus. Not planned; the web UI and Home
  Assistant replace it (documented in README, INSTALL, FAQ).
- **Pre- and post-heaters** (internal EH on PB14, KWL-EM/ERW module): never switched on. A heater
  needs a dedicated safety concept (independent cut-off, bench tests with a dummy load).
- **The stock Modbus TCP server** (port 502): REST and MQTT replace it.
- **easyControls 3.x and other manufacturers:** different hardware; a bridge (Modbus client
  feeding the same UI and Home Assistant crates) would be the way, not replacement firmware.

## 2. To confirm on the unit (first test)

None of these is confirmed yet. The firmware reports most of them itself under *Settings ›
Technical details › Board*.

- **Fans:** which output is supply and which is exhaust (`firmware/src/board.rs`, inferred from
  the stock code), and one tacho pulse per revolution (compare rpm with the stock UI's values).
- **Bypass:** actuator type and direction (*Settings › Bypass actuator*), run time.
- **Sensors:** temperatures against a thermometer. The stock NTC curve reads about 0.5 K low at
  25 °C; it is kept as it is.
- **SD card:** which power polarity worked, the card-detect level.
- **Mains pulses per second** on the power-fail input (50 or 100), and that a short power cut
  saves the counters.
- **Ethernet:** PHY address, link LED.
- **LED polarities** (harmless if inverted).
- **Frost protection** at the first frost: the stock controller keeps cycling the +1.2 V exhaust
  boost while efficiency stays above 59.9 % (kept on purpose; compare with the stock unit).
- **Firmware update from the web UI** and going back to stock with a `.okwl` (section 5).
- **KNX module** on a unit that has one (section 4).
- **Speed:** how long the web UI takes to load (first load and cached), and that a page load
  with several parallel requests works.

## 3. Design notes

### 3.1 SD card write budget

- **Target:** at most about 10 writes a day in normal use.
- **Settings** (`CONFIG.INI`, `SECRETS.INI`, `AUTH.BIN`) are written only when a setting
  changes, 10 s after the last change (at most 60 s later), each twice: first its backup
  (`CONFIG.BAK`, `SECRETS.BAK`, `AUTH.BAK`), then the file itself. A power cut while saving
  leaves one complete copy; an empty `CONFIG.INI` is restored from the backup at boot.
- **Counters and mode** go into a 64-byte record in `STATEA.BIN`/`STATEB.BIN`, written
  alternately in place (no FAT changes). Saved every 6 h, on restart and power failure, and
  about a minute after the mode or manual level changes (at most 10 min later, one write for a
  burst of changes). These prompt mode saves are capped at 6 per day of uptime
  (`MODE_SAVES_PER_DAY` in `openkwl-app`): a Home Assistant automation changing the level every
  15 minutes would otherwise cost about 100 writes a day; beyond the cap the mode waits for the
  next 6-hourly, restart or power-fail save. Any state save also carries a pending mode change.
  A filter reset is saved at once (one write, rare).
- **History** (`openkwl_app::history`): the day in progress stays in RAM (96 quarter-hour
  points and the day's totals, 1.2 KB); one 1216-byte record is written after each local
  midnight into its slot of `DAYS.BIN` (`day % 366` × 2048 bytes, in place): one write a day.
  The first write grows the file with zeros up to the day's slot (up to 0.75 MB, once); a day
  that can't be written within 15 minutes (no card) is dropped. A restart loses the day so
  far; the record then covers less than 24 h (`seconds recorded`).
- **`CONFIG.BAK`** also follows a hand-edited `CONFIG.INI` that loads (at boot, only when it
  differs); a `CONFIG.INI` that doesn't load leaves both files alone.
- **Logs** stay in RAM (`GET /api/v1/log`). Only the `sd-log` debug build writes a log file.
- **Never written:** `WWW/`, firmware images, and the stock files on the card.
- The web server serves only `WWW/`, never `HELIOS/`.

### 3.2 Home Assistant entities

Implemented in `crates/openkwl-ha`, matching HA conventions for ventilation units (the full
list for users: [HOME_ASSISTANT.md](HOME_ASSISTANT.md)):

- **`fan`** is the main entity: on/off, levels 1–4 (`speed_range`), presets `auto`/`party`/
  `quiet`. `climate` would imply a temperature setpoint the unit doesn't have.
- **Holiday** is not a preset (it needs dates); it shows in the *Operating mode* sensor.
- **Sensors:** the four temperatures, efficiency (%), recovered heat power (W) and energy (kWh,
  `total_increasing`, usable in long-term statistics), filter days left.
- **Diagnostics:** rpm, fan voltage, operating hours, frost state.
- **Binary sensors:** bypass (opening), frost (cold), defrost (running), filter due and fault
  (problem, with the fault list as an attribute).
- **Controls:** bypass cooling switch, bypass target number, filter reset button, restart button.
- **Device:** `configuration_url` to the web UI, `connections` with the MAC (merges with router
  integrations), `origin` = openKWL.
- The weekly program and holiday dates stay in the web UI; automations can switch the mode.
- Caveat: the HA switch can turn bypass cooling on without the web UI's rubber-plug warning.

## 4. KNX module support

Facts, as observed in the stock firmware's behaviour. In short:

- The KWL-KNX is a Modbus RTU slave (57600 8N1) on the RS-485 module bus
  (USART1, PB10 = driver enable), address 0x14 (confirmed on a unit, 2026-10-24). PD14 must
  stay an input: driven low it silences the whole bus. Its supply comes from the bus' +24 V, switched by PE8, which
  openKWL holds off today.
- The controller polls it every second and exchanges "blocks" (copies of the stock firmware's
  RAM structs): level and auto flag from KNX; level, mode, time, status, temperatures, filter to
  KNX.

Implemented in new code from the observed stock behaviour (reference frames in
`crates/openkwl-bus/src/knx/tests.rs`), no bus capture:
- `crates/openkwl-bus`: Modbus RTU (FC03/04/16, CRC) and the module protocol: presence with the
  stock back-off, block encoders byte-identical to stock, register 12 → level/Auto change
  (change-only, like stock; levels above 4 are clamped), programme pushes.
- `crates/openkwl-app/src/knx.rs`: snapshot for the blocks, commands and pushes into the app.
- `firmware/src/bus.rs`: bus power (PE8 soft start, PE4, overcurrent on PC2: 4 trips → off,
  fault `bus_overcurrent`), USART1 by register polling with PB10 driver enable.
- API/UI: `modules` in `GET /system/details`, *Technical details › Extension modules*.

Open: confirm on a unit with a KWL-KNX; a simulated KNX slave in `openkwl-sim`.

## 5. Firmware update from the web UI

Implemented with the stock bootloader alone (contract in `firmware/memory.x`). How it
works, the package format and every check: [UPDATING.md](UPDATING.md); the code is in
`crates/openkwl-update`. Open: the first update on a unit, and how long the bootloader really
takes (the UI assumes 60 s).
