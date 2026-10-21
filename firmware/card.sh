#!/bin/sh
# Builds everything for a test card in ../card/ (docs/INSTALL.md):
#   BAS90001.bin  firmware image (the bootloader installs it once, then renames it)
#   settings.ini  your settings.ini with only FwBasis= changed
#   WWW/          the web UI
#   openkwl-fw.elf the matching ELF, needed to decode a debug log
#   THIRD_PARTY_LICENSES.txt  the licences of the crates, npm packages and fonts inside
#   openkwl.okwl  the same firmware and web UI as one update file for Settings › Firmware
#                 update (normal build only; the --sd-log debug build has no update feature)
#
# usage: firmware/card.sh <settings.ini from your original card> [--sd-log]
set -eu
cd "$(dirname "$0")"
mise install --quiet   # the pinned Rust, Node and pnpm (mise.toml), once
settings=${1:?usage: firmware/card.sh <settings.ini> [--sd-log]}
features=""
if [ "${2:-}" = "--sd-log" ]; then features="--no-default-features --features sd-log"; export DEFMT_LOG=info; fi

# shellcheck disable=SC2086
mise exec -- cargo build --release $features
elf=target/thumbv7m-none-eabi/release/openkwl-fw
llvm=$(ls -d "$(mise exec -- rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
"$llvm/llvm-objcopy" -O binary "$elf" target/openkwl-fw.raw

out=../card
rm -rf "$out"
mkdir -p "$out"
(cd .. && mise exec -- cargo run -q -p openkwl-pack -- pack firmware/target/openkwl-fw.raw --version 90001 --out card --settings "$settings")
(cd .. && mise exec -- cargo run -q -p openkwl-pack -- verify card/BAS90001.bin)
# The web UI's packages, once (pnpm-lock.yaml pins them).
[ -d ../web/node_modules ] || (cd ../web && mise exec -- pnpm install --frozen-lockfile)
(cd ../web && mise exec -- pnpm -s package:sd)
cp -R ../dist-sd/WWW "$out/WWW"
cp "$elf" "$out/openkwl-fw.elf"
# The licences of everything the image and the web UI contain, for whoever gets these files.
python3 ../scripts/third-party.py --full "$out/THIRD_PARTY_LICENSES.txt" >/dev/null
if [ -z "$features" ]; then
  (cd .. && mise exec -- cargo run -q -p openkwl-pack -- bundle card/BAS90001.bin dist-sd/WWW --out card/openkwl.okwl)
fi
echo
echo "Ready in card/:"
ls -l "$out"
head -1 "$out/settings.ini"
