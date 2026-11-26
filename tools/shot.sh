#!/usr/bin/env bash
# Screenshot run in the PROCYON_RUN mode (tools/rungame.py): a window on the desktop, or Xvfb + Mesa software Vulkan without a display.
# usage: tools/shot.sh <out.png> [game args...]   e.g. tools/shot.sh local/x.png --shot-frame 300 --script damage
# --shot-frame takes one frame (file: <out.png>) or a list, 100,200,300 (files: <out>_100.png, <out>_200.png, ...). Pictures are 1280x720.
# The game runs from game/, so a relative path argument would silently resolve there: the path of `--mod`, `--check-mod` and
# `--dump-schemas` (the arguments that take a folder) is made absolute against the caller's working directory first. Other
# arguments are passed as given (`--shot` is set here, from <out.png>).
# Prints each saved file and the elapsed time; exits non-zero when a picture is missing or black.
set -uo pipefail
out=$(realpath -m "$1"); shift
root="$(cd "$(dirname "$0")/.." && pwd)"
# The import every tool uses (tools/import_assets.py find_assets: a fresh linked worktree uses the main checkout's).
[ -n "${PROCYON_ASSETS:-}" ] || { a=$(cd "$root" && python3 -m tools.import_assets --print-assets); [ -z "$a" ] || export PROCYON_ASSETS="$a"; }
# This checkout's own mod overlay (the default sits next to the assets, shared by every worktree using the main import).
export PROCYON_OVERLAY="${PROCYON_OVERLAY:-$root/local/tool-overlay}"
# A scratch XDG_DATA_HOME (after the asset lookup, which may use the per-user import): the player's key bindings, options and
# saves neither change the run nor get changed by it.
export XDG_DATA_HOME; XDG_DATA_HOME=$(mktemp -d); trap 'rm -rf "$XDG_DATA_HOME"' EXIT
frames=500; prev=
for a in "$@"; do [ "$prev" = --shot-frame ] && frames=$a; prev=$a; done
expected=()
if [[ "$frames" == *,* ]]; then for f in ${frames//,/ }; do expected+=("${out%.png}_$f.png"); done; else expected=("$out"); fi
rm -f "${expected[@]}"
# Relative folder arguments become absolute against the caller's directory, before `cd`. `--mod=<dir>` is not a form the game reads.
args=(); prev=
for a in "$@"; do
    case "$prev" in
        --mod|--check-mod|--dump-schemas) case "$a" in --*) ;; *) a=$(realpath -m -- "$a") ;; esac ;;
    esac
    args+=("$a"); prev=$a
done
set -- "${args[@]+"${args[@]}"}"
cd "$root/game"
# The `fast` profile (game/Cargo.toml): near-release speed with incremental rebuilds. The built binary runs from game/, as cargo run did.
# PROCYON_BIN (an absolute path) is a binary the caller has built: smokes.sh and check.sh build once and skip the per-run cargo.
if [ -n "${PROCYON_BIN:-}" ]; then game=$PROCYON_BIN
else
    cargo build --profile fast -q || exit 1
    game="${CARGO_TARGET_DIR:-$root/game/target}/fast/openprocyon"
fi
start=$SECONDS
# The run mode (PROCYON_RUN: window, software, gpu-hidden, auto) and its display and graphics settings are decided in one place
# for every tool (tools/rungame.py), which also prints the `run:` line (mode, adapter, frames). PROCYON_JOBS: games running at once.
(cd "$root" && exec python3 -m tools.rungame run -- "$game" --shot "$out" "$@"); code=$?
elapsed=$((SECONDS - start))
[ "$code" -eq 0 ] || { echo "shot.sh: the game exited with status $code" >&2; exit "$code"; }
status=0
for f in "${expected[@]}"; do
    if [ ! -s "$f" ]; then echo "shot.sh: missing picture $f" >&2; status=1; continue; fi
    python3 - "$f" <<'PY' || { echo "shot.sh: $f is black (the game already runs on X11; try PROCYON_RUN=software to rule out the desktop driver)" >&2; status=1; continue; }
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
sys.exit(1 if max(hi for _, hi in im.getextrema()) < 12 else 0)
PY
    echo "shot.sh: saved $f"
done
echo "shot.sh: ${elapsed}s"
exit "$status"
