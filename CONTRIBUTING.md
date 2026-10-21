# Contributing to openKWL

Thank you for helping! openKWL is a hobby project that controls real ventilation units in
people's homes. Contributions are very welcome; the guidelines below keep it safe and
maintainable.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to help

| You can … | How |
|---|---|
| **Test it on your unit** | The most valuable contribution right now. Follow [INSTALL.md](docs/INSTALL.md) and [send a unit report](../../issues/new?template=unit_report.yml), even if everything worked. |
| **Report a bug** | [Bug report](../../issues/new?template=bug_report.yml), with *Settings › Technical details* and the log |
| **Suggest a feature** | [Feature request](../../issues/new?template=feature_request.yml). Check [docs/PLAN.md](docs/PLAN.md) first. |
| **Improve the docs** | Pull requests for typos, unclear steps or missing answers are always welcome |
| **Write code** | See below; for anything bigger than a small fix, open an issue first |
| **Report a security problem** | Privately, never in a public issue: [SECURITY.md](SECURITY.md) |

## Before you write code

1. **Open an issue first** for anything beyond a small fix, so we can agree on the approach
   before you spend time on it. This matters especially for:
   - **the control logic that follows the stock firmware** (`bypass`, `cwp`, `efficiency`, `fan`,
     `frost`, `ntc`, `tacho`, `average` in `openkwl-core`, and `openkwl-auth`). These reimplement
     the stock firmware's observed behaviour (same thresholds and states), quirks included, on
     purpose;
   - **names and IDs that Home Assistant sees.** Changing them breaks users' automations;
   - **anything that writes to the SD card,** because of the write budget and the rule that
     stock files are never changed;
   - **new dependencies,** because of the flash budget.
2. Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (setup, tests, common changes) and
   [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (where things go).

## Development setup

```sh
# needs mise (https://mise.jdx.dev); it installs Rust, Node and pnpm
./dev.sh               # simulator + web UI with hot reload, no hardware needed
```

## Pull requests

- **One topic per pull request.** Small and focused is easier to review.
- **Behaviour goes into the `no_std` crates, with tests.** `firmware/` and `tools/openkwl-sim`
  stay thin.
- **All checks pass** ([DEVELOPMENT.md, Tests and checks](docs/DEVELOPMENT.md#tests-and-checks)):
  `cargo test`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`, the UI tests and
  type check, and the OpenAPI lint.
- **Firmware changes:** the three variants still build and fit the flash slot. Say in the pull
  request how many bytes it adds or saves (`llvm-size`).
- **API changes:** `crates/openkwl-api` and `api/openapi.yaml` change together; then
  `pnpm gen:api`.
- **UI changes:** add a screenshot (light and dark, desktop and phone, if the layout changes).
- **Docs:** update `README.md`, `docs/` and `docs/PLAN.md` when behaviour or status changes, and
  add a line to the *Unreleased* section of [CHANGELOG.md](CHANGELOG.md).
- **Tested on a unit?** Say so, and say which model. If not, say that too; that's fine.

## Style

- **Rust:** `rustfmt` with the repository's `rustfmt.toml`, and clippy-clean. Bounded types only
  (`heapless`); every buffer bound has a worst-case test. Comments explain *why*, briefly.
- **TypeScript:** strict mode; Mantine components with theme defaults; no hard-coded colours or
  sizes.
- **User-facing text:** English, sentence case, plain words for homeowners; buttons start with a
  verb. Use the same names for modes and sensors as in Home Assistant.
- **Commit messages:** a short summary line in the imperative or as a topic
  (`openkwl-ha: hand-written JSON for state and discovery`), then a body that says what and why.

## What not to commit

- Firmware images, stock Helios files (`*.bin`, `*.CWP`, `*.HWP`, …), extracts or disassembly
  of the stock firmware. They are copyrighted by Helios. Describe behaviour in your own words
  and code it anew.
- Your network details, passwords, MAC addresses, serial numbers or other unit identifiers. Use
  the example values from the existing tests.
- Build output (`card/`, `dist-sd/`, `target/`). `.gitignore` covers these.

## Licensing of contributions

openKWL is licensed under the [MIT License](LICENSE). By submitting a contribution you agree
that it is distributed under the same license.

## Questions?

See [SUPPORT.md](SUPPORT.md).
