#!/usr/bin/env bash
# Screenshot run that works with or without a display: uses Xvfb + Mesa software Vulkan when there is no $DISPLAY (Windows always has one).
# usage: tools/shot.sh <out.png> [game args...]   e.g. tools/shot.sh local/x.png --shot-frame 300 --script damage
# --shot-frame takes one frame (file: <out.png>) or a list, 100,200,300 (files: <out>_100.png, <out>_200.png, ...). Pictures are 1280x720.
# Prints each saved file and the elapsed time; exits non-zero when a picture is missing or black (retry with WAYLAND_DISPLAY=).
set -uo pipefail
out=$(realpath -m "$1"); shift
root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "${PROCYON_ASSETS:-}" ] && [ ! -d "$root/assets" ]; then  # fresh linked worktree: use the main checkout's import
    common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
    if [ -n "$common" ] && [ -d "$(dirname "$common")/assets" ] && [ "$(dirname "$common")" != "$root" ]; then
        export PROCYON_ASSETS="$(dirname "$common")/assets"; echo "shot.sh: no assets/ here, using $PROCYON_ASSETS"
    fi
fi
frames=500; prev=
for a in "$@"; do [ "$prev" = --shot-frame ] && frames=$a; prev=$a; done
expected=()
if [[ "$frames" == *,* ]]; then for f in ${frames//,/ }; do expected+=("${out%.png}_$f.png"); done; else expected=("$out"); fi
rm -f "${expected[@]}"
cd "$root/game"
# The `fast` profile (game/Cargo.toml): near-release speed with incremental rebuilds. The built binary runs from game/, as cargo run did.
cargo build --profile fast -q || exit 1
game="${CARGO_TARGET_DIR:-$root/game/target}/fast/openprocyon"
start=$SECONDS
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || [ "$(uname -o 2>/dev/null)" = Msys ]; then
    "$game" --shot "$out" "$@"; code=$?
else
    xvfb-run -a -s "-screen 0 1280x720x24" env WGPU_BACKEND=vulkan "$game" --shot "$out" "$@"; code=$?
fi
elapsed=$((SECONDS - start))
[ "$code" -eq 0 ] || { echo "shot.sh: the game exited with status $code" >&2; exit "$code"; }
status=0
for f in "${expected[@]}"; do
    if [ ! -s "$f" ]; then echo "shot.sh: missing picture $f" >&2; status=1; continue; fi
    python3 - "$f" <<'PY' || { echo "shot.sh: $f is black; on Wayland retry with WAYLAND_DISPLAY=" >&2; status=1; continue; }
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
sys.exit(1 if max(hi for _, hi in im.getextrema()) < 12 else 0)
PY
    echo "shot.sh: saved $f"
done
echo "shot.sh: ${elapsed}s"
exit "$status"
