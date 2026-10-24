# AGENTS.md: working on openKWL

Instructions for coding agents and contributors. Read `README.md` (status),
`docs/ARCHITECTURE.md` (structure, decisions) and `docs/PLAN.md` (open work, design notes) first;
human contributors also `CONTRIBUTING.md` and `docs/DEVELOPMENT.md`.

## What this is
Replacement firmware in Rust for the Helios easyControls ventilation controller (STM32F2x7,
Cortex-M3, behind the stock bootloader at `0x08008000`), with a React/Mantine web UI, a
REST API and Home Assistant MQTT integration. It reimplements the stock firmware's observed
behaviour (same thresholds and states) in new code; no Helios code or binaries are included,
and none may be added. The behaviour is documented where it is implemented: module docs in
`openkwl-core` and `openkwl-bus`, pin sources in `firmware/src/board.rs`, the bootloader
contract in `firmware/memory.x` and `docs/UPDATING.md`. Stock values captured from a unit:
`tools/openkwl-import/testdata/`.

## Architecture rules
- All device behaviour lives in `no_std`, allocation-free crates tested on the host:
  `openkwl-core` (logic), `openkwl-api` (REST types), `openkwl-auth`, `openkwl-config` (files),
  `openkwl-ha` (MQTT/HA), `openkwl-bus` (Modbus RTU and the KWL-KNX protocol), `openkwl-update`
  (update packages, image checks), and `openkwl-app`,
  which ties them together (control loop + HTTP handlers + persistence decisions).
- `firmware/` is only the hardware layer around `openkwl-app`; board facts live in `firmware/src/board.rs`.
  `tools/openkwl-sim` is the host platform for the same code; keep both platforms thin.
- Bounded types only (`heapless`). JSON out via `serde-json-core`; JSON in via the small reader
  `openkwl_api::json` (a hand-written `FromJson` per request type, tested against serde, about
  9 KB smaller than serde's generated parsers): a new request type needs a `FromJson` and a
  case in `json/tests.rs`. Every `*_JSON_MAX` / buffer bound
  needs a worst-case test.

## Commands
Run everything through mise (pinned toolchain):

```sh
./dev.sh                                      # simulator + UI (hot reload), see --help
scripts/vendor.sh                             # once after cloning (dev.sh does it): patched serde-json-core
mise exec -- cargo test                       # all Rust tests
mise exec -- cargo clippy --workspace --all-targets
mise exec -- cargo build -p openkwl-app --target thumbv7m-none-eabi   # no_std check (also core/api/auth/config/ha/bus/update)
(cd firmware && mise exec -- cargo build --release && mise exec -- cargo clippy --release)   # device image
(cd firmware && mise exec -- cargo clippy --release --no-default-features --features sd-log)   # also: --features rtt
cd web && mise exec -- pnpm test              # UI unit tests (vitest)
cd web && mise exec -- npx tsc -p . --noEmit  # type check
cd web && mise exec -- pnpm gen:api           # regenerate src/api/schema.d.ts after changing api/openapi.yaml
mise exec -- pnpm --package=@redocly/cli@latest dlx redocly lint api/openapi.yaml
```

Clippy must be clean (CI fails on warnings). Where a lint is wrong on purpose (the NaN
handling in `fan.rs` and `bypass.rs`, two in `openkwl-auth`), allow it on that one statement
with the reason.

## Rules
- **Interactive by default:** state changes must reach the web UI and Home
  Assistant within about a second. The UI polls `/status` every 1 s while visible; MQTT
  publishes via `App::ha_state_due` (switching values at once, measurements beyond their dead
  band at most every 30 s, fan speed/voltage every 60 s, heartbeat 5 min). New state or entities must take part in
  `openkwl_ha::State::change_from`; never rely on the heartbeat alone.
- **Control loop priority:** `control::control_task` runs in a higher-priority interrupt
  executor. It must never touch the SD card (use `storage::request`) or the embassy-net stack
  (read the `net::NETWORK` snapshot); both are only safe from the normal executor.
- **Patched crate:** `vendor/serde-json-core` is not committed. `scripts/vendor.sh` downloads
  upstream 0.6.0 (checksum-verified) and applies `vendor/serde-json-core.patch` (smaller float
  parser and writer, about 15 KB less flash, `vendor/README.md`). Change the patch, not the
  generated directory. Floats elsewhere go through `openkwl_core::num::parse_f32`; never
  call `str::parse::<f32>` in device code, it brings core's 10 KB table back.
- **Bootloader contract** (`firmware/memory.x`): initial SP in 0x2001xxxx, image ≤ 0x37E00 +
  CRC trailer, never touch flash sectors 0–1 (bootloader, stock settings).
- **Do not change the behaviour of `openkwl-core`'s stock-behaviour modules** (`bypass`, `cwp`,
  `efficiency`, `fan`, `frost`, `ntc`, `tacho`, `average`) **or of `openkwl-auth` without
  agreeing it with the maintainers in an issue first.** They reimplement the stock firmware's
  observed behaviour (same thresholds and states), quirks included. New behaviour goes into
  new modules or `openkwl-app`.
- **The API contract has two halves:** `crates/openkwl-api` and `api/openapi.yaml`. Change both;
  the test `crates/openkwl-api/tests/openapi.rs` fails when they drift. Then run
  `pnpm gen:api`. `openkwl-api` has `#![warn(missing_docs)]`: keep it warning-free.
- **SD card:**
  - 8.3 file names only (`embedded-sdmmc`).
  - openKWL owns `HELIOS/`, `WWW/`, `WWW2/` and its images `BAS90001.BIN`/`BAS90002.BIN`;
    **never modify or delete stock files** (`Web*/`, `*.CGP`, `*.CWP`, `*.HGP`, `*.HWP`, other
    `BAS*.bin`, `settings.ini`, logs). Rollback depends on them.
  - The one deliberate exception: an armed firmware update (`openkwl-update`)
    rewrites the first line of `settings.ini` (`FwBasis=`), after a one-time copy to
    `HELIOS/SETTINGS.BAK`. Nothing else in `settings.ini` changes.
  - Respect the write budget (`docs/PLAN.md` §3.1).
- **A real unit is read-only for agents:** only GET and the stock `xml=` read requests. Never log
  in, never POST settings, never call `/logout`, never flash. Changes go through the simulator.
- **Security:**
  - every `/api/v1` endpoint except `/setup` and `/auth/login` requires a session;
  - never echo secrets (MQTT password, password hashes);
  - the static file server must never serve `HELIOS/`.
- Don't put machine-specific paths, credentials or anyone's network details into code;
  the live-unit fixtures in `tools/openkwl-import/testdata/` are the deliberate exception.
- **Firmware size budget:** the release image must fit `firmware/memory.x` (224 KB, the stock
  slot) in all three variants (default, `sd-log`, `rtt`); currently a few KB of headroom in the
  default build (`firmware/README.md`, *Budgets*). Measure (`llvm-size -A`, `llvm-nm --size-sort -C`)
  before adding dependencies. No `core::fmt` float formatting or panic messages in release builds
  (`firmware/README.md`, *Budgets*).
- **Shared running simulators:** a developer may have their own `./dev.sh` running on port 8080
  with `sim-sd/`. For automated checks, start a separate instance on another port with a copy of the card.
- **Docs:** keep `README.md` (status), `docs/ARCHITECTURE.md` (crates and decisions),
  `docs/PLAN.md` and the user-facing guides in `docs/` (`USER_GUIDE`, `HOME_ASSISTANT`, `API`,
  `UPDATING`, `FAQ`) up to date when behaviour or status changes, and add a line to the
  *Unreleased* section of `CHANGELOG.md`. Screenshots in `docs/images/` come from the simulator.

## UI conventions (`web/`)
- **Design system KWL** (rules in `CLAUDE.md`). `web/src/theme/` is the source of truth for
  colours, sizes and component defaults; change the theme there, and extend in `src/lib/`
  (e.g. `readings.ts` adds percent/power/energy/voltage kinds to `formatReading`).
- Mantine **9** (React ≥ 19.2). The theme was written for Mantine 7 and type-checks unchanged
  on 9; Mantine 9 renamed `Grid gutter` to `gap` and `Collapse in` to `expanded`. Check
  https://mantine.dev/llms.txt for v9 APIs. Mantine 9 writes `data-disabled="false"` on an
  enabled SegmentedControl; theme selectors on `[data-disabled]` must exclude `="false"`.
- Pages: **Overview** (`/`), **History** (`/history`, `/history/live|day|month`), **Schedule**
  (`/schedule/:section`) and **Settings** (`/settings/:section`), path routes in
  `src/lib/router.ts` with redirects from the old routes; the firmware and the simulator answer every path without a `.` with `index.html`.
  Navigation: header tabs from 640 px, a bottom tab bar below. Schedule and Settings use
  `SectionList`; each section is a `SettingsSection` with a save bar. Page titles read
  "Page · openKWL". `/design` (only in `pnpm dev`) shows every AirflowSchematic and Overview state.
- Mode names in Home Assistant stay **Auto, Manual, Party, Quiet, Holiday** (`openkwl-ha`
  presets and mode sensor). The web UI never shows "Manual" as a control: choosing Off/1–4 in
  the Mode control is manual control. The four air temperatures keep the technical names
  Outdoor / Supply / Extract / Exhaust air in both.
- **Languages (react-i18next):** English and German, the browser's language by default,
  *Settings › Language* per browser. All UI text lives in `web/src/i18n/locales/<lang>/<area>.json`
  (English is the reference, keys are type-checked; `locales.test.ts` fails if German misses a key
  or placeholder). Components: `useTranslation('<area>')`; plain functions: `t` from `src/i18n`.
  Numbers, dates and times only via `src/i18n/format.ts` / `reading()` (Intl, browser region),
  never hard-coded `'en-GB'`. German uses "du" and Helios' terms (Stufe, Zuluft, Abluft,
  Fortluft, Außenluft, Party, Ruhe, Urlaub, Manuell); Home Assistant names stay English. Check
  German on a phone: it is longer. The "Copy as text" report in Technical details stays English.
- **Feedback in place, no toasts** (`components/Feedback.tsx`): a save bar says "✓ Saved" for
  a moment; a button with its own action shows `loading`, then `ActionDone` next to it, and a
  failure as `ActionError` in the same section; the Overview's controls show their new state, and a
  failed command in the hint line. Keep layouts from jumping when feedback appears.
- Charts: `@mantine/charts` (Recharts) on the *History* page, colours from the theme; it is the
  biggest part of `app.js` (about 130 KB gzipped).
- Custom CSS: only the theme's `mantine-overrides.css` classes and
  `components/AirflowSchematic.css`; everything else uses Mantine props and theme tokens.
- Fonts: Geist and Geist Mono ship in `public/fonts/` with `OFL.txt` (SIL Open Font License;
  keep it beside the fonts). Units follow a no-break space (Geist has no narrow-space glyph).
- The web UI is on the **SD card** (`WWW/`, 8.3 names), not in flash. Long names (icons,
  `/fonts/…`) are mapped in `scripts/package-sd.mjs` and `firmware/src/http.rs` (`ALIASES`);
  keep both in sync. Everything is gzipped except the `.WF2` fonts.
- The build must stay `index.html` + `app.js` + `app.css` plus the icons and fonts from `public/`.
  Caching: `index.html` asks for `/app.js?v=<content hash>` and `/app.css?v=…` (plugin in
  `vite.config.ts`); the firmware serves `?v=` URLs and fonts as immutable, `index.html` as
  `no-cache`, icons for an hour and the API as `no-store`.

## Environment quirks
- `openapi-typescript` needs the TypeScript 5 compiler API; `pnpm gen:api` runs it via `dlx`
  with TS 5 while the project uses TS 7.
- The simulator's SD card is `sim-sd/` (git-ignored); `./dev.sh --setup` resets its password.
