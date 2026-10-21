# Patched crates

## serde-json-core 0.6.0

Only the patch is in this repository: `serde-json-core.patch`. `scripts/vendor.sh` (run by
`./dev.sh` and CI; run it yourself once after cloning) downloads the upstream crate from
crates.io, checks it against the SHA-256 that crates.io publishes, converts its CRLF line
endings to LF, applies the patch and puts the result in `serde-json-core/` (git-ignored). It
does nothing when that directory is already up to date with the patch.

Upstream source (MIT/Apache-2.0, licence files included in the download), changed as follows:

- `src/float.rs` (new) and one line in `src/de/mod.rs` (`deserialize_fromstr!`): JSON numbers are
  parsed with a small decimal parser instead of `core`'s `f32/f64::from_str`. That saves about
  13 KB of flash, which the firmware slot (223.5 KB, fixed by the stock bootloader) cannot spare.
  The parser is the same as `openkwl_core::num::parse_f32`, which is tested there against `core`.
- `src/lib.rs`: `mod float;`; `src/de/mod.rs`: the now unused `use core::str::FromStr;` removed.
- `src/float.rs` (`format_f32`) and the `serialize_ryu!` macro and `serialize_f64` in
  `src/ser/mod.rs`: floats are written by a small in-house writer instead of `ryu`, which
  saves about 2.3 KB of flash (ryu's code and tables). The output is the same as ryu's: the
  shortest decimal that reads back as the same `f32` (the nearest one if there are several,
  ties to even), in ryu's layout (`21.0`, `-8.25`, `0.00001`, `1e-7`, `1.2345679e-12`,
  `1e13`); non-finite values are still `null`. It works with exact 192-bit integer
  comparisons and no tables, so it is slower than ryu (up to about 100 comparisons for a
  9-digit value). `f64` is written as the nearest `f32` (exact for every `f64` that is an
  `f32`; openKWL serialises no `f64`). `Cargo.toml`: `ryu` moved to `[dev-dependencies]`.
  The unit tests in `src/float.rs` compare with ryu; the ignored one checks all 2^32 bit
  patterns. openkwl-config's INI writer uses the same writer (via `to_slice`).

Wired up with `[patch.crates-io]` in `Cargo.toml` and `firmware/Cargo.toml`.

To change the patch: edit `serde-json-core/` (after `scripts/vendor.sh`), then regenerate the
patch against a fresh, LF-converted upstream copy `a/` with your edited copy as `b/`:
`diff -ruN -x .patched a b > vendor/serde-json-core.patch` (strip the dates from the `---`/`+++`
lines). To move to a new upstream release, change `version` and `sha256` in
`scripts/vendor.sh` (the checksum is the `cksum` in https://index.crates.io/se/rd/serde-json-core)
and update the patch.

The crate is not a workspace member, so `cargo test -p serde-json-core` does not work. To run
its tests, copy the directory elsewhere, append `[workspace]` to the copy's `Cargo.toml` and
run `RUSTFLAGS=--cap-lints=warn cargo test --release` there (upstream's tests trip its own
`#![deny(warnings)]` on a deprecated constant); add `-- --ignored` for the all-`f32` check
(about 15 minutes on 8 cores).
