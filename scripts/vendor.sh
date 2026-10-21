#!/usr/bin/env bash
# Fetches serde-json-core 0.6.0 from crates.io, checks it against the checksum crates.io
# publishes, and applies vendor/serde-json-core.patch (smaller float code, vendor/README.md).
# The result, vendor/serde-json-core, is not committed: run this once after cloning, and again
# when the patch changes (it does nothing if the directory is up to date). Needs curl.
set -euo pipefail
cd "$(dirname "$0")/.."

name=serde-json-core
version=0.6.0
sha256=5b81787e655bd59cecadc91f7b6b8651330b2be6c33246039a65e5cd6f4e0828
patch_file=vendor/$name.patch
dest=vendor/$name

hash() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

stamp="$version $(hash "$patch_file")"
if [[ -f $dest/.patched && $(cat "$dest/.patched") == "$stamp" ]]; then
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/crate.tar.gz" "https://static.crates.io/crates/$name/$name-$version.crate"
if [[ $(hash "$tmp/crate.tar.gz") != "$sha256" ]]; then
  echo "$name-$version.crate: checksum mismatch, not using it" >&2
  exit 1
fi
tar -xzf "$tmp/crate.tar.gz" -C "$tmp"
src=$tmp/$name-$version
# Some upstream files have CRLF line endings; the patch is made against LF.
find "$src" -type f -exec perl -pi -e 's/\r\n/\n/' {} +
patch -s -p1 -d "$src" < "$patch_file"
echo "$stamp" > "$src/.patched"
rm -rf "$dest"
mv "$src" "$dest"
echo "$dest: $name $version from crates.io, patched"
