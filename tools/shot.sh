#!/usr/bin/env bash
# Screenshot run that works with or without a display: uses Xvfb + Mesa software Vulkan when there is no $DISPLAY (Windows always has one).
# usage: tools/shot.sh <out.png> [game args...]   e.g. tools/shot.sh local/x.png --shot-frame 300 --script damage
set -euo pipefail
out=$(realpath -m "$1"); shift
cd "$(dirname "$0")/../game"
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || [ "$(uname -o 2>/dev/null)" = Msys ]; then exec cargo run --release -- --shot "$out" "$@"; fi
[[ " $* " == *" --late-frame "* ]] || set -- --late-frame 0 "$@"  # the frame-9000 shot takes ~10 min in software
exec xvfb-run -a -s "-screen 0 1280x720x24" env WGPU_BACKEND=vulkan cargo run --release -- --shot "$out" "$@"
