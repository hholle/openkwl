# Development guide

Everything you need to build, test and change openKWL. Read
[ARCHITECTURE.md](ARCHITECTURE.md) first for the big picture. The project rules (also for coding
agents) are in [AGENTS.md](../AGENTS.md).

## Prerequisites

- **[mise](https://mise.jdx.dev).** It installs the pinned toolchain from `mise.toml`: Rust
  1.99.1 with the `thumbv7m-none-eabi` target, clippy, rustfmt and llvm-tools; Node 22.23.3;
  pnpm 10.34.6. Exact versions on purpose: a new compiler can change whether the firmware fits.
  Run every command through `mise exec --`, or activate mise in your shell.
- macOS or Linux. Windows should work under WSL; it is untested.
- For hardware work, optionally: an SWD probe and
  [probe-rs](https://probe.rs); `defmt-print` (`cargo install defmt-print`) for the SD-card log.

You don't need a ventilation unit: the simulator runs the same application code.

After cloning, run `scripts/vendor.sh` once (`./dev.sh` does it for you). It downloads
`serde-json-core` from crates.io, checks its checksum and applies our small patch
(`vendor/serde-json-core.patch`, see [vendor/README.md](../vendor/README.md)); Cargo can't
resolve the workspace without it.

## Repository layout

```
api/openapi.yaml        REST API contract (kept in sync with crates/openkwl-api by a test)
crates/                 no_std libraries: all behaviour, tested on the host
  openkwl-core           control logic (stock behaviour + new modes, schedule, clock, …)
  openkwl-api            REST types and validation
  openkwl-auth           passwords, sessions, throttle
  openkwl-config         CONFIG.INI, SECRETS.INI, AUTH.BIN, STATEA.BIN/STATEB.BIN
  openkwl-ha             Home Assistant discovery, state, commands, MQTT codec
  openkwl-bus            Modbus RTU and the KWL-KNX protocol
  openkwl-update         .okwl update packages
  openkwl-app            the device application that ties them together
firmware/               Embassy firmware for the STM32F207 (own Cargo project)
tools/
  openkwl-sim            simulator: the app + a thermal model + HTTP, on your computer
  openkwl-import         reads settings from a stock unit (read-only)
  openkwl-pack           verifies and packs firmware images, bundles .okwl files
web/                    openKWL web UI (React 19, Mantine 9, Vite)
vendor/                 patch for serde-json-core (smaller float code), applied by scripts/vendor.sh
docs/                   documentation
dev.sh                  simulator + UI in one command
```

## Day-to-day loop

```sh
./dev.sh                     # simulator on :8080 + UI with hot reload on http://localhost:5173
```

- Changes in `web/` reload in the browser at once.
- Changes in the Rust crates need a restart of `./dev.sh`.
- The simulated SD card is `sim-sd/` (git-ignored). On first start it is filled from the captured
  stock settings in `tools/openkwl-import/testdata`. Delete `sim-sd/` to start fresh.

| Command | Does |
|---|---|
| `./dev.sh --setup` | Forget the simulator's password and start with the setup wizard |
| `./dev.sh --built` | Build the UI once and serve everything from the simulator on :8080, as on the unit |
| `./dev.sh --import-live <url> [--cwp <file>]` | Fill `sim-sd/` from a unit that still runs the stock firmware (read-only, no login); `--cwp` adds the parameter file from its SD card |
| `./dev.sh -- --speed 60` | Run the simulation 60× faster |
| `./dev.sh -- --outdoor -8 --fixed-outdoor --icing` | Watch frost protection and defrosting |
| `./dev.sh -- --no-clock` | Start as before the first time sync |

The simulator's options (`cargo run -p openkwl-sim -- --help`):

```
--sd <dir>          simulated SD card (default: sim-sd)
--port <n>          HTTP port (default: 8080)
--www <dir>         serve the web UI build from here (e.g. web/dist)
--speed <n>         simulated seconds per real second (default: 1)
--outdoor <°C>      mean outdoor temperature (default: 12)
--fixed-outdoor     keep the outdoor temperature constant instead of a daily curve
--icing             let the heat exchanger ice up below -3 °C (try --outdoor -8)
--no-clock          start without a valid clock (as before the first NTP sync)
--setup             forget the password and start in setup mode
```

> [!TIP]
> For automated checks, run a second simulator on another port with a copy of the card, for
> example `--port 8097 --sd /tmp/sim-copy`. That way you don't disturb a `./dev.sh` that is
> already running.

## Tests and checks

Run these before you open a pull request:

```sh
mise exec -- cargo test                                         # all Rust tests (host)
mise exec -- cargo clippy --workspace --all-targets             # lints
mise exec -- cargo fmt --check                                  # formatting of the workspace (rustfmt.toml)
(cd web && mise exec -- pnpm test)                              # UI tests (vitest)
(cd web && mise exec -- npx tsc -p . --noEmit)                  # UI type check
mise exec -- pnpm --package=@redocly/cli@latest dlx redocly lint api/openapi.yaml
```

The `no_std` crates must also build for the device:

```sh
for c in openkwl-core openkwl-api openkwl-auth openkwl-config openkwl-ha openkwl-bus openkwl-update openkwl-app; do
  mise exec -- cargo build -p $c --target thumbv7m-none-eabi
done
```

Clippy must be clean; CI fails on warnings. A lint that is wrong on purpose (the NaN handling
in `fan.rs` and `bypass.rs`) is allowed on that one statement, with the reason.

## Building the firmware

```sh
cd firmware
mise exec -- cargo build --release                                           # normal build
mise exec -- cargo clippy --release
mise exec -- cargo clippy --release --no-default-features --features sd-log  # debug build
mise exec -- cargo clippy --release --no-default-features --features rtt     # probe build
```

`firmware/card.sh <settings.ini> [--sd-log]` builds a complete SD card in `card/`, plus
`card/openkwl.okwl` for the normal build. The variants, debugging on the unit, the watchdog and
the memory budgets are in [firmware/README.md](../firmware/README.md).

### The flash budget

The image must fit the stock slot: **223.5 KB**. All three variants must fit. The normal build
has only a few KB to spare ([firmware/README.md](../firmware/README.md), *Budgets*). Before you add a dependency or a feature, measure:

```sh
llvm=$(ls -d "$(mise exec -- rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
$llvm/llvm-size -A target/thumbv7m-none-eabi/release/openkwl-fw          # sections
$llvm/llvm-nm --size-sort -C target/thumbv7m-none-eabi/release/openkwl-fw | tail -30   # biggest symbols
```

No `core::fmt` float formatting and no panic messages in release builds; never
`str::parse::<f32>` in device code (use `openkwl_core::num::parse_f32`). Each of these costs
10 KB or more. [firmware/README.md](../firmware/README.md), section *Budgets*, lists the measures taken so far, and the next candidates.

## Common changes

### Adding or changing an API endpoint

The contract has two halves, and a test fails when they drift:

1. Change the types in `crates/openkwl-api` (keep `#![warn(missing_docs)]` clean).
2. Change `api/openapi.yaml` to match.
3. Handle the route in `crates/openkwl-app/src/http.rs`, with a test in `crates/openkwl-app/src/tests.rs`.
4. `cd web && mise exec -- pnpm gen:api` regenerates `web/src/api/schema.d.ts`.
5. Use it in the UI through `web/src/api/client.ts`.

### Adding a setting

1. Add it to the config type in `crates/openkwl-api`, and to the INI reader and writer in
   `crates/openkwl-config` (with a round-trip test).
2. Add a buffer-size test if it makes `CONFIG.INI` longer (`*_JSON_MAX` and the INI bounds).
3. Expose it in the UI's settings section.

### Home Assistant entities

Entities are defined in `crates/openkwl-ha/src/lib.rs` (`ENTITIES`). New state must take part in
`State::change_from`, so it is published within a second. **Names and IDs that Home Assistant
sees are user-facing:** changing them breaks people's dashboards and automations. Discuss it in
an issue first.

### The control logic

The modules that follow the stock firmware (`bypass`, `cwp`, `efficiency`, `fan`, `frost`,
`ntc`, `tacho`, `average` in `openkwl-core`, and `openkwl-auth`) reimplement its observed
behaviour (same thresholds and states) in new code, quirks included. No Helios code or
binaries go into the repository; describe the behaviour in the module docs. **Don't change their behaviour without discussing it in an issue
first.** New behaviour goes into new modules or `openkwl-app`.

### The web UI

- Use Mantine components with the theme's defaults: never hard-coded colours, sizes or radii.
  `web/src/theme/` is the source of truth for the theme; change colours and component
  defaults there, not in components.
- Show readings with `formatReading()`.
- Copy is English, sentence case, in plain words for homeowners. Buttons start with a verb.
  Modes and sensors have the same names as in Home Assistant.
- Anything that stops the ventilation needs a confirm dialog that says for how long.
- No external requests: fonts and icons ship with the UI, because the unit may have no internet.
- Details: [AGENTS.md, UI conventions](../AGENTS.md#ui-conventions-web).

## Working with a real unit

- **The stock unit** (before installing) is read-only for tools: `openkwl-import` uses only GET
  requests and never logs in.
- **Install and go back** only with the [SD-card procedure](INSTALL.md).
- **Don't commit** firmware images (`*.bin`), the stock parameter files, or anything with your
  network details or unit identifiers. `.gitignore` blocks the common ones.

## Releasing

A release controls ventilation in other people's homes, so it goes out only after it has run on
a real unit. Until several models have been reported working, mark releases on GitHub as
**pre-release**.

**Before tagging**
1. CI is green on the commit (tests, lint, all three firmware variants, flash budget, UI, API).
2. Version in `Cargo.toml` (`[workspace.package]`) and `firmware/Cargo.toml`; the *Unreleased*
   entries in [CHANGELOG.md](../CHANGELOG.md) moved under the new version and date.
3. `scripts/third-party.py` after dependency changes (updates `THIRD_PARTY.md`).
4. Build: `firmware/card.sh <settings.ini>`.

**Hardware gate**, on a unit, with this exact build:
1. First installation by SD card works (or, for an update release, the web UI update from the
   previous release): the version shows under *Technical details*.
2. The fans follow levels 1–4, the four temperatures are plausible, the bypass moves.
3. Going back works: a `.okwl` (or the SD card) with the original image brings the stock
   firmware back with its settings.
4. Power cut while the bootloader flashes: the next power-on flashes again and the unit comes back.
5. Restart and power cut during normal operation: settings and counters survive.
6. At least a few days of operation without unexpected restarts (*Technical details › Last
   reset*), Home Assistant connected.

**Publishing**
1. Tag `vX.Y.Z` and create the GitHub release (pre-release while models are untested). Attach:
   - `openkwl-X.Y.Z.okwl`, the update file (`card/openkwl.okwl`);
   - `BAS90001.bin` and a zip of `WWW/`, for a first installation by SD card;
   - `openkwl-fw.elf`, to decode debug logs and crash addresses;
   - `THIRD_PARTY_LICENSES.txt` (`card/`): required, the licences of everything inside.

   Never attach stock Helios images or parameter files.
2. The release notes repeat the warnings from the README (experimental, own risk, which units).
