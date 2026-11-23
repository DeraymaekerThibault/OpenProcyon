#!/bin/sh
# The validation gate, in tiers. usage: tools/check.sh [--quick | --full [--allow-partial]]
#   --quick  while iterating (seconds): ruff and the hygiene rules always; rustfmt + clippy when Rust changed, the Python tests
#            when Python changed ("changed" = against the merge base with origin/main, plus uncommitted and untracked files).
#   default  before a commit: ruff, hygiene, Python tests, rustfmt, clippy, Rust tests (asset tests too when an import exists).
#   --full   before a push or PR: default + the `fast` game build, the startup smoke and the battle check. Fails without imported
#            assets unless --allow-partial.
# Cheap checks run first. Each step prints one line; its full log is local/check/<step>.log. The last line is GATE OK, GATE FAIL
# or GATE PARTIAL (something was skipped for lack of assets). Needs ruff (pip install -r requirements-dev.txt); a .venv in the
# checkout is used when present. Clippy and tests build in the dev profile, the game in `fast` (game/Cargo.toml).
set -e
tier=default allow_partial=
for a in "$@"; do
  case $a in
    --quick) tier=quick ;;
    --full) tier=full ;;
    --fast) tier=default ;; # the old name of the default tier
    --allow-partial) allow_partial=1 ;;
    *) echo "usage: tools/check.sh [--quick | --full [--allow-partial]]" >&2; exit 2 ;;
  esac
done
cd "$(dirname "$0")/.."
# The checkout's virtualenv first (bin/ on Linux and macOS, Scripts/ on Windows, where python3 may be the Store stub).
for bin in .venv/bin .venv/Scripts; do [ -d "$bin" ] && PATH="$PWD/$bin:$PATH"; done
command -v ruff >/dev/null || { echo "ruff not found: pip install -r requirements-dev.txt" >&2; exit 1; }
# Cargo turns incremental builds off when CI is set; locally (also under agents that set CI) the tiers rely on them.
[ -n "${GITHUB_ACTIONS:-}" ] || unset CI
logs=local/check; mkdir -p "$logs"
start=$(date +%s)

# step <name> <command...>: runs the command with its output in local/check/<name>.log and prints one line. On failure: the
# first 20 relevant lines, the command to rerun, and GATE FAIL.
step() {
  name=$1; shift
  t=$(date +%s)
  if "$@" >"$logs/$name.log" 2>&1; then
    printf 'ok   %-14s %4ss\n' "$name" "$(($(date +%s) - t))"
  else
    printf 'FAIL %-14s %4ss\n' "$name" "$(($(date +%s) - t))"
    { grep -E -i -m 20 'error|warning|panicked|FAIL|Traceback|would reformat|Diff in' "$logs/$name.log" || tail -n 20 "$logs/$name.log"; } | cut -c1-200
    echo "rerun: $*"
    echo "GATE FAIL $name ($logs/$name.log)"
    exit 1
  fi
}

rust=1 python=1
if [ "$tier" = quick ] && base=$(git merge-base HEAD origin/main 2>/dev/null || git rev-parse HEAD 2>/dev/null); then
  changed=$( { git diff --name-only "$base"; git ls-files -o --exclude-standard; } 2>/dev/null)
  printf '%s\n' "$changed" | grep -q -E '^game/|\.rs$' || rust=
  printf '%s\n' "$changed" | grep -q -E '\.py$' || python=
fi

step ruff-format ruff format --check .
step ruff-lint ruff check .
step hygiene python3 -m unittest discover -s tests -p test_repo_hygiene.py
[ -z "$python" ] || step python-tests python3 -m unittest discover -s tests
if [ -n "$rust" ]; then
  step rustfmt cargo fmt --manifest-path game/Cargo.toml --check
  step clippy cargo clippy --manifest-path game/Cargo.toml --all-targets -- -D warnings
fi
if [ "$tier" = quick ]; then
  skipped=${rust:+x}${python:+y}  # x: Rust checked, y: Python tests ran
  case $skipped in x) note=" (no Python change: Python tests skipped)" ;; y) note=" (no Rust change: rustfmt, clippy skipped)" ;;
    "") note=" (no Rust or Python change: rustfmt, clippy, Python tests skipped)" ;; *) note= ;; esac
  echo "GATE OK quick $(($(date +%s) - start))s$note"
  exit 0
fi

imported=$(python3 -c 'import sys; sys.path.insert(0, "tests"); from imported import ASSETS; print(int((ASSETS / "ships").is_dir()))')
if [ "$imported" = 1 ]; then
  step rust-tests cargo test --manifest-path game/Cargo.toml -- --include-ignored
else
  step rust-tests cargo test --manifest-path game/Cargo.toml
fi
skipped=
if [ "$tier" = full ]; then
  if [ "$imported" = 1 ]; then
    step game-build cargo build --manifest-path game/Cargo.toml --profile fast
    game="${CARGO_TARGET_DIR:-$PWD/game/target}/fast/openprocyon"
    # Startup smoke: the unit tests run single systems, so only a real start finds conflicts between systems (Bevy B0001) and
    # startup panics. Runs the menu for 10 s (timeout ends it with status 124); only a panic in the log fails. Needs a display
    # or Xvfb (like tools/shot.sh).
    startup_smoke() {
      if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || [ "$(uname -o 2>/dev/null)" = Msys ]; then
        out=$(cd game && timeout 10 "$game" 2>&1) || true
      else
        out=$(cd game && xvfb-run -a -s '-screen 0 1280x720x24' env WGPU_BACKEND=vulkan timeout 10 "$game" 2>&1) || true
      fi
      printf '%s\n' "$out"
      ! printf '%s\n' "$out" | grep -q "panicked at"
    }
    step startup-smoke startup_smoke
    # Battle check: short skirmishes with fixed fleets must still play like battles (the computer engages, nobody grinds on an
    # island, nothing panics). Whole-battle regressions that single-system tests miss show up here (~1-2 min).
    step battle-check python3 -m tools.battle_check
  else
    skipped="startup smoke, battle check, "
  fi
fi
total="$(($(date +%s) - start))s"
if [ "$imported" = 1 ]; then
  echo "GATE OK $tier $total"
else
  echo "GATE PARTIAL $tier $total (no assets: ${skipped}Rust asset tests skipped; import with tools/build_assets.sh --game <install>)"
  [ "$tier" != full ] || [ -n "$allow_partial" ] || { echo "--full needs imported assets (or --allow-partial)" >&2; exit 1; }
fi
