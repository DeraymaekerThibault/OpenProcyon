#!/bin/sh
# The validation gate, in tiers. usage: tools/check.sh [--quick | --full [--allow-partial]]
#   --quick  while iterating (seconds): ruff and the hygiene rules always; rustfmt + clippy when Rust changed, the Python tests
#            when Python changed ("changed" = against the merge base with origin/main, plus uncommitted and untracked files).
#   default  before a commit: ruff, hygiene, Python tests, rustfmt, clippy, Rust tests (asset tests too when an import exists).
#   --full   before a push or PR: default + the `fast` game build, the startup smoke, the battle check and the replay check. Fails without imported
#            assets unless --allow-partial.
# Cheap checks run first (the Python tests run beside the Rust steps). Each step prints one line; its full log is local/check/<step>.log. The last line is GATE OK, GATE FAIL
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
# python3 when it runs (not the Windows Store stub, not absent: the Windows venv only has `python`), else python.
py=python3; "$py" -c '' >/dev/null 2>&1 || py=python
command -v ruff >/dev/null || { echo "ruff not found: pip install -r requirements-dev.txt" >&2; exit 1; }
# Cargo turns incremental builds off when CI is set; locally (also under agents that set CI) the tiers rely on them.
[ -n "${GITHUB_ACTIONS:-}" ] || unset CI
logs=local/check; mkdir -p "$logs"
start=$(date +%s)

# report <name> <status> <seconds> <command...>: the one line of a step; on failure also the first 20 relevant lines of its log and
# the command to rerun (the caller prints GATE FAIL).
report() {
  rname=$1 rstatus=$2 rsecs=$3; shift 3
  if [ "$rstatus" = 0 ]; then
    printf 'ok   %-14s %4ss\n' "$rname" "$rsecs"
  else
    printf 'FAIL %-14s %4ss\n' "$rname" "$rsecs"
    { grep -E -i -m 20 'error|warning|panicked|FAIL|Traceback|would reformat|Diff in' "$logs/$rname.log" || tail -n 20 "$logs/$rname.log"; } | cut -c1-200
    echo "rerun: $*"
  fi
}

# step <name> <command...>: runs the command with its output in local/check/<name>.log and prints one line. On failure: the
# first 20 relevant lines, the command to rerun, and GATE FAIL (after the background step's own line, if one is running).
step() {
  name=$1; shift
  t=$(date +%s)
  if "$@" >"$logs/$name.log" 2>&1; then status=0; else status=1; fi
  if [ "$status" != 0 ]; then bg_join; fi
  report "$name" "$status" "$(($(date +%s) - t))" "$@"
  [ "$status" = 0 ] || { echo "GATE FAIL $name ($logs/$name.log)"; exit 1; }
}

# bg_start <name> <command...>: like step, but runs beside the following steps; bg_join prints its line (and, on failure, the
# excerpt and rerun command) and sets bg_failed. Nothing is printed before the join, so the lines stay one per step.
bgpid= bgname= bgcmd= bg_failed=
bg_start() {
  bgname=$1; shift; bgcmd="$*"
  ( t=$(date +%s)
    if "$@" >"$logs/$bgname.log" 2>&1; then status=0; else status=1; fi
    echo "$status $(($(date +%s) - t))" >"$logs/$bgname.status" ) &
  bgpid=$!
}
bg_join() {
  [ -n "$bgpid" ] || return 0
  wait "$bgpid" || true
  bgpid=
  read -r bstatus bsecs <"$logs/$bgname.status"
  report "$bgname" "$bstatus" "$bsecs" $bgcmd
  [ "$bstatus" = 0 ] || bg_failed=1
}
# bg_finish: join, and stop the gate when the background step failed.
bg_finish() {
  bg_join
  [ -z "$bg_failed" ] || { echo "GATE FAIL $bgname ($logs/$bgname.log)"; exit 1; }
}

rust=1 python=1
if [ "$tier" = quick ] && base=$(git merge-base HEAD origin/main 2>/dev/null || git rev-parse HEAD 2>/dev/null); then
  changed=$( { git diff --name-only "$base"; git ls-files -o --exclude-standard; } 2>/dev/null)
  printf '%s\n' "$changed" | grep -q -E '^game/|\.rs$' || rust=
  printf '%s\n' "$changed" | grep -q -E '\.py$' || python=
fi

# The imported assets, found like every tool finds them (tools/import_assets.py --print-assets: also the main checkout's from a
# linked worktree) and exported so the game and the battle check read the same folder.
assets=$($py -m tools.import_assets --print-assets)
# The game builds its mod overlay next to the assets (<assets>/../local/overlay); with the main checkout's import every worktree
# would rebuild the same folder under the others' runs, so each checkout gets its own.
export PROCYON_OVERLAY="${PROCYON_OVERLAY:-$PWD/local/tool-overlay}"
if [ -n "$assets" ]; then imported=1; export PROCYON_ASSETS="$assets"; echo "ASSETS: $assets"
else imported=0; echo "ASSETS: missing"; fi
step ruff-format ruff format --check .
step ruff-lint ruff check .
step hygiene $py -m unittest discover -s tests -p test_repo_hygiene.py
# The Python tests run beside rustfmt, clippy and the Rust tests (they start no compiler); the hygiene rules ran above, so the
# discover run is told to skip them (tests/test_repo_hygiene.py).
[ -z "$python" ] || bg_start python-tests env PROCYON_HYGIENE_DONE=1 $py -m unittest discover -s tests
if [ -n "$rust" ]; then
  step rustfmt cargo fmt --manifest-path game/Cargo.toml --check
  step clippy cargo clippy --manifest-path game/Cargo.toml --all-targets -- -D warnings
fi
if [ "$tier" = quick ]; then
  skipped=${rust:+x}${python:+y}  # x: Rust checked, y: Python tests ran
  case $skipped in x) note=" (no Python change: Python tests skipped)" ;; y) note=" (no Rust change: rustfmt, clippy skipped)" ;;
    "") note=" (no Rust or Python change: rustfmt, clippy, Python tests skipped)" ;; *) note= ;; esac
  bg_finish
  echo "GATE OK quick $(($(date +%s) - start))s$note"
  exit 0
fi

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
    # startup panics. Opens the main menu, writes one picture after 120 frames and quits by itself (shot.sh: Xvfb when there is
    # no display; a panic, a missing or a black picture fails).
    export PROCYON_BIN=$game
    startup_smoke() { tools/shot.sh local/check/startup.png --menu main --shot-frame 120; }
    step startup-smoke startup_smoke
    # Battle check: short skirmishes with fixed fleets must still play like battles (the computer engages, nobody grinds on an
    # island, nothing panics). Whole-battle regressions that single-system tests miss show up here (~1-2 min).
    step battle-check $py -m tools.battle_check
    # Replay check: skirmishes on the normal clock with scripted orders record a command log (PROCYON_COMMANDS), and their replay
    # (--replay) must give the same battle report (game/src/commands.rs, ~2 min). border_dispute_full is launched through the lobby
    # and replays from the log's skirmish setup alone.
    step replay-check $py -m tools.battle_check red_rover border_dispute_full --replay
  else
    skipped="startup smoke, battle check, replay check, "
  fi
fi
bg_finish
total="$(($(date +%s) - start))s"
if [ "$imported" = 1 ]; then
  echo "GATE OK $tier $total"
else
  echo "GATE PARTIAL $tier $total (no assets: ${skipped}Rust asset tests skipped; import with tools/build_assets.sh --game <install>)"
  [ "$tier" != full ] || [ -n "$allow_partial" ] || { echo "--full needs imported assets (or --allow-partial)" >&2; exit 1; }
fi
