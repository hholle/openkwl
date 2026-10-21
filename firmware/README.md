# firmware/: openKWL for the STM32F2x7 main controller

The hardware layer around [`openkwl-app`](../crates/openkwl-app). Every behaviour lives in the
`no_std` crates and is tested on the host, and `tools/openkwl-sim` runs the same code. This
crate only connects it to the peripherals.

> **First booted on one unit on 2026-10-17** (KWL EC 300 W R, STM32F207VCT6) and has run there
> for a few days, with no frost season yet. Pins, clocks and the PHY were determined from how
> the stock firmware drives the board, each with its source in [`src/board.rs`](src/board.rs);
> details still marked *inferred* there are listed in [`docs/PLAN.md`](../docs/PLAN.md) §2.
> The firmware update from the web UI has not run on a unit yet. Trying it:
> [`docs/INSTALL.md`](../docs/INSTALL.md).

## Build

```sh
cd firmware
mise exec -- cargo build --release            # → target/thumbv7m-none-eabi/release/openkwl-fw (ELF)
mise exec -- cargo clippy --release
```

Three variants; only one fits the stock slot with everything in it:

| Build | Features | For |
|---|---|---|
| `cargo build --release` | `update` (default) | the unit: firmware update from the web UI, no log output, a panic restarts at once (`nolog.rs`) |
| `--no-default-features --features sd-log` | `sd-log` | debugging without a probe: log on the SD card, `BOOT.LOG`; **no firmware update** (no room) |
| `--no-default-features --features rtt` | `rtt` | debugging with an SWD probe (RTT log); no firmware update |

A test card (image, `settings.ini`, web UI, matching ELF) in one step; the normal build also
gets `card/openkwl.okwl`, the update file for *Settings › Firmware update*:

```sh
firmware/card.sh <settings.ini from your card> [--sd-log]   # → card/ (docs/INSTALL.md)
mise exec -- cargo run -p openkwl-pack -- bundle <image.bin> [<www dir>] --out <file.okwl>  # by hand
```

`openkwl-pack verify` checks the CRC trailer and that the vector table belongs at `0x08008000`.

The crate is outside the Cargo workspace because it builds only for `thumbv7m-none-eabi`
(see `.cargo/config.toml`).

## Debugging on the unit

What the firmware tells you without any tools:

- **The LEDs while it starts** (after the bootloader's blinking; PD5 and PD6 are the two LEDs
  the bootloader blinks too). Steady states, so wherever it stops stays visible:

  | LEDs | Reached |
  |---|---|
  | both dark | our image never runs (the bootloader did not jump to it) |
  | only PD6 | it runs, but writing the top of RAM (the stack) locks the core |
  | both on | RAM test passed; stopped in `main` before the clocks were up |
  | only PD5 | clocks up; stopped at the outputs, RTC, SD card or settings |
  | PD6 blinking once a second | running (PD5 on = SD card OK, off = no card) |
  | both blinking fast | the top of RAM does not keep what is written |

- **`HELIOS/BOOT.LOG`** on the card (`sd-log` build only), plain text, one line per step and
  start (seconds since the start): `start, reset flags …, card mounted`, `settings loaded`,
  `running`, `network up, <address>`. Several `start` lines without `running` = a restart loop; the reset flags tell why
  (bits: 0x20 watchdog, 0x10 software restart, 0x08 power-on, 0x04 reset pin, 0x02 brown-out;
  a power-on usually shows 0x0e).

- **Settings › Technical details** in the web UI: the last reset cause (power-on, watchdog,
  software, pin, brown-out), uptime, stack high-water mark, SD card, network, MQTT, and the
  *Board* findings (card-detect level, SD power polarity, mains pulses per second, PHY address).
  Two hardware faults the unit runs on with:
  - **RTC clock `LSI`**: the 32.768 kHz crystal did not start within about 3 s, so the RTC runs
    from the internal oscillator (it can be off by tens of percent; SNTP sets it again on every
    sync, without a network the clock and the schedule drift). The
    switch reset the backup domain once (RTC time, the bootloader's backup registers).
  - **PHY `no 50 MHz clock`**: the Ethernet MAC gets no RMII reference clock (dead PHY or
    oscillator). The network stays off until the next start; ventilation, saves and the module
    bus run on. `BOOT.LOG` says `network off: no 50 MHz reference clock`.
- **The log** (`GET /api/v1/log`, *Show log* under Technical details): the app's last 4 KB of
  events (logins, mode changes, saves, bypass, faults), in plain text.

**With an SWD probe** (ST-Link, J-Link on the controller's SWD pins, unit off mains): build with
`DEFMT_LOG=info mise exec -- cargo build --release --no-default-features --features rtt` (the
normal build logs nothing). Attach without reflashing and watch the log live:

```sh
probe-rs attach --chip STM32F207VCTx target/thumbv7m-none-eabi/release/openkwl-fw
```

`DEFMT_LOG=debug` (environment, at build time) adds the debug messages; `.cargo/config.toml`
sets `off` for the normal build. `--features panic-messages` prints the panic text and place, but the
image no longer fits the stock slot, so it only runs when flashed directly with the probe.
Halting the core also stops the watchdog (`DBGMCU`), so stepping does not reset the unit.

**Without a probe: log to the SD card.**

```sh
mise exec -- cargo build --release --no-default-features --features sd-log
```

- Every `defmt` message goes to a 2 KB RAM ring instead of RTT. Every 5 s the storage task
  appends it to `HELIOS/DEFMT.LOG` (restarted empty beyond 4 MB).
- The ring is in `.uninit` RAM: what was not written yet survives a watchdog reset, a panic
  or a hard fault. Panics and hard faults (with `pc` and `lr`) are logged, then the unit
  restarts, and the message reaches the card after the restart.
- **Keep the ELF of exactly that build.** The file holds compact defmt frames, not text.
  Decode it on the computer (`cargo install defmt-print` once):

  ```sh
  defmt-print -e target/thumbv7m-none-eabi/release/openkwl-fw < /Volumes/<card>/HELIOS/DEFMT.LOG
  ```
- A hard fault's `pc`/`lr` map to source lines with
  `llvm-addr2line -e target/thumbv7m-none-eabi/release/openkwl-fw -f -C <pc>`.
- The panic message itself has no location (all panic sites' file names would need ~20 KB of
  flash); the messages before it usually show where it happened.
- It writes to the card a lot more than normal: for bench sessions, not for daily use.

## Watchdog

- The independent watchdog starts first thing in `main` with plain register writes (10 s, like
  stock), before the clocks are set up, so even a hang there restarts the unit. Then
  `IndependentWatchdog` takes over with 8 s (`board::WATCHDOG_US`); `main` feeds it between
  the SD card attempts and after reading the settings. From then on
  only the control loop feeds it, once a second, from the high-priority executor.
- It feeds only while the normal executor (network, HTTP, MQTT, SD card) also makes progress:
  `control::alive_task` counts there every second. If that count stops for 30 s (a hung
  Ethernet DMA, SD card or future), the control loop stops feeding, and the unit restarts 8 s
  later. Password hashing and SD writes block that executor for a second or two at most.
- A hang of the control loop itself, or of an interrupt above it, stops the feeding directly.
- Restart and power failure wait at most 3.5 s and 1.6 s, well inside the 8 s.
- The reset cause is in *Technical details* and in the first log line (`reset flags`).

## Structure

| Module | Does | Logic it uses (host-tested) |
|---|---|---|
| `main.rs` | Clocks (HSE 25 MHz → 120 MHz, LSE), safe output states, interrupts, task start | |
| `board.rs` | Pin map and constants with their source, bootloader version (RTC BKP1) | |
| `sysinfo.rs` | Reset cause (RCC_CSR), chip IDs, image/RAM layout, stack high-water mark | |
| `io.rs` | ADC round-robin + NTC, tacho periods via EXTI, TIM1 PWM, fan enables, bypass drive, LEDs, power-fail watch (PB3 zero-cross); the tacho and power-fail tasks run in the high-priority executor, so SD writes cannot delay their edge timing | `openkwl_core::{ntc, tacho, average}` |
| `control.rs` | 1 Hz loop in a **higher-priority interrupt executor** (UART4): RTC → `App::tick_second` → outputs, hands writes to the storage task, reboot, watchdog, power-fail | `openkwl_app::App` |
| `storage.rs` | SD power (polarity tried both ways), SDIO + FAT (`embedded-sdmmc`), volume, root and web folder kept open: boot load, storage task (prepare under the app lock, write without it; re-initialises the card after errors and writes again), web files streamed from an open handle (`WWW/`, or `WWW2/` when its marker names the running image), stock `settings.ini` (read; only an armed update rewrites its first line), factory MAC from OTP; the 12 KB `BIG` buffer for `CONFIG.INI` is shared with HTTP | `openkwl_config` |
| `net.rs`, `phy.rs` | Ethernet RMII with the KSZ8031/8081 PHY (reset, address scan, 50 MHz mode) + DHCP/static | |
| `http.rs` | Port 80, 3 workers: `/api/v1` → `App::handle_hashed` (password hashing with nothing locked; config import/export and the log via the shared `BIG` buffer), gzip files from `WWW/` streamed with a 4 KB send window | `openkwl_app::http` |
| `update.rs` | Only with `--features update` (default): `PUT /api/v1/system/update` streamed onto the card while checked; web folder pairing and the `settings.ini` switch in `storage.rs` | `openkwl_update` |
| `sdlog.rs` | Only with `--features sd-log`: defmt to a RAM ring that survives resets, appended to `HELIOS/DEFMT.LOG` | |
| `nolog.rs` | The normal build: defmt goes nowhere, a panic restarts at once | |
| `mdns.rs` | Answers `<hostname>.local` (multicast 224.0.0.251:5353), announces on every new address | `openkwl_core::mdns` |
| `sntp.rs` | Time sync → RTC; configured server, else the router, else pool.ntp.org | `openkwl_core::sntp` |
| `mqtt.rs` | Home Assistant: discovery, state (checked every second via `App::ha_state_due`, published on change), commands, last will, keep-alive ping every 30 s | `openkwl_ha`, `openkwl_ha::mqtt`, `App::ha_command` |

## Budgets (release build, 2026-10-21)

| | Used | Limit | |
|---|---|---|---|
| Flash | about 216.4 KB with the update (the `sd-log` and `rtt` variants were last measured before the JSON reader: 222.3 and 220.6 KB) | 223.5 KB (`memory.x`: 0x37E00, the bootloader's image area) | about 97 % |
| RAM | 95.2 KB static | 128 KB; the stack starts at 0x2001FF00 (bootloader rule) | 74 % |
| Stack | about 23 KB at boot, about 25 KB for an HTTP request | about 29.3 KB (what the statics leave; 27.9 KB with `sd-log`) | |

The flash limit is fixed by the bootloader: it erases sectors 2–5 and checks the CRC over
0x08008000–0x0803FDFF only ([`memory.x`](memory.x)). Size measures already
in place:
- **No panic message formatting:** about 37 KB, the biggest single item.
- **Own float writer for JSON and INI** (`vendor/README.md`) instead of `core::fmt`: about
  12 KB; instead of `ryu` (with `small`, which itself saved about 10 KB): about 2.3 KB.
- **No `embedded-sdmmc` logging:** about 3 KB.
- **No `core::fmt` for small strings** (KNX firmware version, 8.3 names): a few hundred bytes
  each; the `sd-log` build logs routine events (saves, mDNS) at `debug`, which it leaves out.
- **Hand-written JSON for MQTT** (`openkwl-ha/src/json.rs`): the state and discovery documents
  go through a few shared calls instead of serde's per-field code; a test checks the output
  is byte-identical to serde's. About 0.4 KB.
- **USART1 by plain register polling** (`bus.rs`) instead of embassy's DMA driver: the module
  bus runs at 57 600 baud with 60-byte frames, so polling costs little time and saves flash.

Check the limit with all three variants (`card.sh … --sd-log` builds with `DEFMT_LOG=info`).
The normal build is the fullest: the firmware update (about 3 KB) fits because that build has
no log output (2.5 KB), no `BOOT.LOG` (0.8 KB), and the stock-image scan of *Technical details*
was dropped (1.1 KB).

About 7 KB free in the normal build (2026-10-21) since request bodies are read by `openkwl_api::json` instead of serde's per-type parsers (about 8.9 KB); `json()`/`parse()` in `openkwl-app` stay out of line (inlined they cost 2.2 KB). Next candidates:
mostly `serde-json-core` code per config type; a shared non-generic helper may save a few KB.

**Stack:** there is no guard below it, so an overflow silently corrupts statics. Build large
values in their static, never as a temporary: `ConstStaticCell` for `const fn` values (the
Ethernet queue), statics for big buffers (`storage::BIG`), and keep boot work in
`#[inline(never)]` functions so it does not stay in `main`'s async frame. Check
the frames after changes: `llvm-objdump -d` and look for large `sub sp, sp, #…`.

`opt-level = "z"` with fat LTO is smaller than `"s"` for this code. Measure with
`llvm-size -A` / `llvm-nm --size-sort -C` before adding dependencies.

## Known gaps

- **No network without the RMII clock:** the reference clock is checked once at start (`net.rs`);
  a PHY that starts delivering it later needs a restart.
- **Host name:** mDNS and DHCP use the host name from the start; a new one takes effect after a
  restart, like the other network settings (a new address from DHCP is announced within 5 s).
- **Size limits on the card:** a `CONFIG.INI` longer than 12 KB counts as damaged (the backup
  is used, *settings invalid*); a firmware update refuses to arm when `settings.ini` is longer
  than 512 bytes.
- **DHCP option 42** (time server announced by the router): embassy-net's DHCP socket consumes
  the replies, so the firmware uses the router (gateway) as the time server instead.
- **KNX module:** only the KWL-KNX at address 0x14 is driven (as stock); other modules on the
  bus are not. The bus is polled, a module that comes back later is found again.
