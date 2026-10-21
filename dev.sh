#!/usr/bin/env bash
# Development environment without hardware: the firmware simulator (openkwl-sim, which runs
# the real device code) plus the web UI.
#
#   ./dev.sh                  simulator on :8080 + UI with hot reload on :5173
#   ./dev.sh --built          build the UI once; the simulator serves everything on :8080
#   ./dev.sh --setup          start with the setup wizard (forgets the simulator's password)
#   ./dev.sh --import-live <url> [--cwp <file.CWP>]
#                             first fill the simulated SD card from a unit running the stock
#                             firmware (read-only, no login), e.g. http://192.168.1.50/; --cwp adds
#                             the unit's parameter file from its SD card (fan voltages, limits)
#   ./dev.sh -- <sim args>    extra simulator options, e.g. -- --outdoor -8 --icing --speed 60
#
# The simulated SD card lives in ./sim-sd (git-ignored). On first start it is filled from the
# captured stock settings in tools/openkwl-import/testdata.
set -euo pipefail
cd "$(dirname "$0")"

built=0
setup=0
import_live=
cwp=
sim_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --built) built=1 ;;
    --setup) setup=1 ;;
    --import-live) import_live=${2:?--import-live needs the address of the unit, e.g. http://192.168.1.50/}; shift ;;
    --cwp) cwp=${2:?--cwp needs a file}; shift ;;
    --) shift; sim_args+=("$@"); break ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 1 ;;
  esac
  shift
done

if ! command -v mise >/dev/null; then
  echo "mise is required (https://mise.jdx.dev); it provides the pinned Rust, Node and pnpm." >&2
  exit 1
fi
run() { mise exec -- "$@"; }

scripts/vendor.sh   # patched serde-json-core (once; vendor/README.md)

echo "==> toolchain"
run rustc --version
run node --version

if [[ ! -d web/node_modules ]]; then
  echo "==> installing web dependencies"
  (cd web && run pnpm install --frozen-lockfile)
fi

if [[ -n $import_live ]]; then
  echo "==> importing settings from the stock unit at $import_live (read-only)"
  run cargo run -q -p openkwl-import -- --from "$import_live" --out sim-sd ${cwp:+--cwp "$cwp"}
elif [[ ! -f sim-sd/HELIOS/CONFIG.INI ]]; then
  echo "==> creating sim-sd from the captured stock settings"
  run cargo run -q -p openkwl-import -- --from-dir tools/openkwl-import/testdata --out sim-sd
fi

echo "==> building the simulator"
run cargo build -q -p openkwl-sim

args=(--sd sim-sd --port 8080)
[[ $setup -eq 1 ]] && args+=(--setup)
if [[ $built -eq 1 ]]; then
  echo "==> building the web UI"
  (cd web && run pnpm build)
  args+=(--www web/dist)
fi

./target/debug/openkwl-sim "${args[@]}" ${sim_args[@]+"${sim_args[@]}"} &
sim=$!
trap 'kill $sim 2>/dev/null; wait $sim 2>/dev/null' EXIT INT TERM

if [[ $built -eq 1 ]]; then
  echo "==> open http://localhost:8080/  (Ctrl-C to stop)"
  wait $sim
else
  echo "==> UI with hot reload on http://localhost:5173/ (API from the simulator on :8080; Ctrl-C to stop)"
  (cd web && run pnpm dev)
fi
