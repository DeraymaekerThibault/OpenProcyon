#!/bin/sh
# The validation gate: format, lint, repository hygiene and tests. usage: tools/check.sh
# Needs ruff (pip install -r requirements-dev.txt).
# Asset-free Rust tests always run; ignored asset tests, the startup smoke and the battle check also run when an import exists.
set -e
cd "$(dirname "$0")/.."
command -v ruff >/dev/null || { echo "ruff not found: pip install -r requirements-dev.txt" >&2; exit 1; }
ruff format --check .
ruff check .
imported=$(python3 -c 'import sys; sys.path.insert(0, "tests"); from imported import ASSETS; print(int((ASSETS / "ships").is_dir()))')
cd game
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
if [ "$imported" = 1 ]; then
  cargo test --release -- --include-ignored
  # Startup smoke: the unit tests run single systems, so only a real start finds conflicts between systems (Bevy B0001) and
  # startup panics. Runs the menu for 10 s; needs a display or Xvfb (like tools/shot.sh).
  cargo build --release
  run=""
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || run="xvfb-run -a -s '-screen 0 1280x720x24' env WGPU_BACKEND=vulkan"
  # timeout ends the game with status 124; only a panic in the log is a failure
  if eval "$run timeout 10 target/release/openprocyon" 2>&1 | grep -q "panicked at"; then
    echo "startup smoke failed: the game panicked in the menu" >&2
    exit 1
  fi
  # Battle check: short skirmishes with fixed fleets must still play like battles (the computer engages, nobody grinds on an
  # island, nothing panics). Whole-battle regressions that single-system tests miss show up here (~1-2 min).
  (cd .. && python3 -m tools.battle_check)
else
  cargo test --release
  echo "NOTE: Asset-dependent Rust tests and startup smoke skipped: no imported game assets (run tools/build_assets.sh --game <install>)." >&2
fi
cd ..
python3 -m unittest discover -s tests
