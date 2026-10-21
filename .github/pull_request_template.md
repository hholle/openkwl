## What and why

<!-- What does this change, and why? Link the issue: "Closes #123". -->

## How it was tested

<!-- Tests added or changed; simulator; on a unit (which model?). "Not tested on a unit" is fine, just say so. -->

## Checklist

- [ ] `cargo test`, `cargo clippy --workspace --all-targets` and `cargo fmt --check` pass
- [ ] UI: `pnpm test` and the type check pass (if `web/` changed)
- [ ] Firmware: all three variants build and fit the flash slot; size change: <!-- e.g. +120 B -->
- [ ] API: `crates/openkwl-api` and `api/openapi.yaml` changed together, `pnpm gen:api` run
- [ ] No change to the reimplemented stock behaviour (frost, bypass, fans, sensors) or to Home Assistant names/IDs, or it was agreed in an issue
- [ ] Docs and the *Unreleased* section of `CHANGELOG.md` updated
- [ ] No firmware images, stock files, passwords or personal network details in the diff

## Screenshots

<!-- For UI changes: before/after, light and dark, desktop and phone if the layout changes. -->
