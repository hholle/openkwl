# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0.0,
minor versions may contain incompatible changes to the API, the settings file or Home Assistant
entities; they are marked **Breaking**.

## [Unreleased]

First public version, in preparation. First booted on one KWL EC 300 W R on 2026-10-17 and has
run there for a few days, with no frost season yet. Not yet tried on a unit: the firmware update
from the web UI and the KNX module.

Licensed under the MIT License.

### Fixed

- **KWL-KNX not found:** openKWL asked address 0x14, but the stock firmware runs the KNX block
  protocol with address 0x19. It now asks 0x19 first and 0x14 in turn until one answers; the log
  names the address it was found at.
- **Temperatures read about 7 K too low:** the sensor supply measured on PC3 is about 3.2 V,
  not 2.5 V; openKWL ignored the measurement as implausible and converted with 2.5 V. It now
  always uses the measurement like the stock firmware (3.2 V only for the first 2 s after start).

### Added

- **Temperature sensor diagnostics** in *Technical details › Board*: the sensor supply measured on
  PC3 and the one the conversion uses, and each sensor's raw ADC value and resistance
  (`GET /system/details`, `board.ntc_supply_mv`, `ntc_supply_used_mv`, `ntc_raw`).
- **Module bus diagnostics** in *Technical details › Extension modules*: bytes received, line
  errors and the last bytes received; the log notes when the KNX module is found or lost and the
  first unusable answers. The receiver is off while the controller sends.
- **Own look:** the web UI uses its own petrol accent colour and logo, not the manufacturer's
  brand colours, so it can't be mistaken for an official product.
- **Safety notice** once after setup in the web UI: experimental, check fans and air direction,
  not with room-air-dependent fireplaces or stoves, not unattended in deep frost.
- **Defaults for new installations:** MQTT base topic `openkwl`; Home Assistant shows the unit's
  own model (imported from the stock settings) instead of a fixed one.
- **Frost settings limited to sane ranges** (defrost at most 4 h, boost delay and test run at
  most 1 h, exhaust boost at most 2 V): defrosting stops the supply fan while the exhaust runs.
  A `CONFIG.INI` with values outside them doesn't load (the backup is used).
- **Firmware** for the STM32F207 main controller (Embassy), installed by the stock bootloader
  from the SD card. The control logic reimplements the stock firmware's observed behaviour
  (same thresholds and states) in new code: NTC sensors, fan voltages, tacho and stall
  detection, efficiency, bypass, frost protection, CWP parameter files. No Helios code or
  binaries are included.
- **openKWL web UI** (React 19, Mantine 9), served from the SD card: overview with mode control
  and an air-flow diagram, weekly schedule editor, holiday, Party and Quiet, settings, technical
  details. Light and dark mode; works on phones.
- **REST API** `/api/v1` with an OpenAPI 3.1 contract; PBKDF2 passwords, session tokens with an
  optional "stay logged in", and a login throttle.
- **Home Assistant** over MQTT 3.1.1 with discovery: 29 entities, changes published within about
  a second.
- **Weekly program** with 5 profiles, Holiday, Party, Quiet, and Back to Auto (at midnight or
  after a set time). A level, mode or preset during a holiday is refused (API 422; Home
  Assistant and KNX ignore it) instead of switching to manual behind the holiday.
- **Heat recovery figures:** efficiency, recovered power and energy, and an air-flow estimate from
  the fan speed.
- **Filter reminder**, bypass season, NTP with EU summer time, mDNS (`<name>.local`).
- **Settings and counters on the SD card** within a write budget: settings files saved
  power-cut safe (backup first), counters and mode alternately in `STATEA.BIN`/`STATEB.BIN`,
  prompt mode saves capped at 6 a day, a filter reset saved at once.
- **KWL-KNX extension module** support, as in the stock firmware (not yet tested on a unit).
- **Firmware and web UI updates from the web UI** with one checked `.okwl` file
  (`openkwl-pack bundle`, `firmware/card.sh`).
- **NTC supply measurement** on PC3, as in the stock firmware.
- **Tools:** `openkwl-sim` (simulator running the same application code), `openkwl-import` (reads
  the settings from a stock unit, read-only), `openkwl-pack` (verify and pack images, bundle
  updates).
- **Debug builds:** `sd-log` (log on the SD card, `BOOT.LOG`) and `rtt` (SWD probe). Start-up
  stages are shown as LED states.
- **Documentation:** installation with a second SD card, user guide, Home Assistant, API,
  updating, architecture, development, FAQ, security policy, contribution guidelines.

[Unreleased]: ../../commits/main
