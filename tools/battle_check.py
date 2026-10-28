"""Battle check: play short skirmish battles with fixed fleets and fail when one stops behaving like a battle.
usage: python3 -m tools.battle_check [NAME ...] [--speed N]   (default: every scenario below)
Each scenario runs the release game with PROCYON_REPORT (game/src/smoke/report.rs), which logs one `battle report {json}` line
when one side is gone or the time is up. The expectations below are whole-battle invariants (the computer engages, nobody grinds on
an island, nothing panics): they catch regressions where one feature quietly changes another (armed volcanoes taken for bases,
map-script tags hiding the captain's target). Runs take ~10-30 s each; logs go to local/battle_check/<name>.log.
Needs imported assets and a display, or Xvfb + Mesa like tools/shot.sh."""

import argparse
import contextlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
GAME = (
    Path(os.environ.get("CARGO_TARGET_DIR", REPO / "game/target"))
    / "release/openprocyon"
)
ANSI = re.compile(r"\x1b\[[0-9;]*m")
REPORT = re.compile(r"battle report (\{.*\})")
FLEETS = ["--ship", "Ship_Navy_Cutter", "--allies", "Ship_Navy_Cutter",
          "--enemies", "Ship_Pirate_Barque,Ship_Pirate_Sloop"]  # fmt: skip
# Seconds a ship may stay in contact with scenery without a break before it counts as grinding.
GRIND = 3.0
# Frames over 50 ms a battle may have (after the first second) before it counts as stuttering; healthy runs have 0-1.
STUTTER = 10

# name, map, game seconds, expectations. "fight": both sides fire and hit, the computer's first burst within 60 s.
# "patrol": every computer ship sails at least 300 units (does not park). Every scenario also checks for panics and grinding.
SCENARIOS = [
    ("ambush", "ambush", 120, {"fight"}),  # starts 514 apart among 22 islands
    ("bayles_stand", "bayles_stand", 120, {"fight"}),  # starts 464 apart
    (
        "diablo_strait",
        "diablo_straight",
        120,
        {"patrol"},
    ),  # armed volcanoes, starts 861 apart
    ("border_dispute", "border_dispute", 120, {"patrol"}),  # armed bases
]


def evaluate(report, expect):
    """The failed expectations of one battle report (empty when it passes)."""
    fails = []
    sides = report["sides"]
    if "fight" in expect:
        for name in ("player", "side1"):
            line = sides.get(name, {})
            if not line.get("bursts"):
                fails.append(f"{name} never fired")
            elif not line.get("hits"):
                fails.append(f"{name} never hit")
        first = sides.get("side1", {}).get("first_burst")
        if first is not None and first > 60:
            fails.append(f"the computer first fired at {first:.0f} s")
    if "patrol" in expect:
        for s in report["ships"]:
            if s["side"] != "player" and s["sailed"] < 300:
                fails.append(f"{s['class']} sailed only {s['sailed']:.0f} units")
    frames = report.get("frame_ms", {})
    if frames.get("over_50", 0) > STUTTER:
        fails.append(
            f"stutter: {frames['over_50']} frames over 50 ms (worst {frames['worst']:.0f} ms)"
        )
    for s in report["ships"]:
        if s["worst_contact"] > GRIND:
            fails.append(
                f"{s['class']} ({s['side']}) ground on scenery for {s['worst_contact']} s"
            )
    return fails


def outcome(log, expect):
    """(passed, summary) for one run's log text."""
    if "panicked at" in log:
        return False, "PANIC"
    m = REPORT.search(log)
    if not m:
        return False, "no battle report (crash or wall-clock timeout)"
    report = json.loads(m[1])
    fails = evaluate(report, expect)
    sides = " ".join(
        f"{k}: {v['bursts']} bursts {v['hits']} hits"
        for k, v in report["sides"].items()
    )
    return not fails, "; ".join(fails) or f"ok at {report['time']:.0f} s ({sides})"


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("names", nargs="*", help="scenarios to run (default: all)")
    ap.add_argument("--speed", type=float, default=16)
    args = ap.parse_args()
    chosen = [s for s in SCENARIOS if not args.names or s[0] in args.names]
    if not chosen:
        sys.exit(f"no scenario named {args.names}; known: {[s[0] for s in SCENARIOS]}")
    subprocess.run(
        ["cargo", "build", "--release", "--quiet"], cwd=REPO / "game", check=True
    )
    out = REPO / "local/battle_check"
    out.mkdir(parents=True, exist_ok=True)
    wrap = (
        []
        if os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")
        else ["xvfb-run", "-a", "-s", "-screen 0 1280x720x24"]
    )
    failed = 0
    with tempfile.TemporaryDirectory() as xdg:
        # A scratch XDG_DATA_HOME (default bindings, no saves, silent) and X11, as tools/autoplay.py.
        (Path(xdg) / "openprocyon").mkdir()
        (Path(xdg) / "openprocyon/settings.json").write_text(json.dumps({"master": 0}))
        env = {**os.environ, "XDG_DATA_HOME": xdg, "WAYLAND_DISPLAY": ""}
        if wrap:
            env["WGPU_BACKEND"] = "vulkan"
        for name, map_id, seconds, expect in chosen:
            log = out / f"{name}.log"
            env["PROCYON_REPORT"] = str(seconds)
            cmd = [
                *wrap,
                str(GAME),
                "--map",
                map_id,
                *FLEETS,
                "--speed",
                str(args.speed),
            ]
            # Wall-clock cap for a hung run (below ~10 fps the game runs slower than asked); a timeout reports no battle.
            with log.open("w") as f, contextlib.suppress(subprocess.TimeoutExpired):
                subprocess.run(
                    cmd,
                    cwd=REPO / "game",
                    env=env,
                    stdout=f,
                    stderr=subprocess.STDOUT,
                    timeout=seconds + 120,
                )
            passed, summary = outcome(
                ANSI.sub("", log.read_text(errors="replace")), expect
            )
            failed += not passed
            print(f"{'ok  ' if passed else 'FAIL'} {name:<16} {summary}", flush=True)
    if failed:
        sys.exit(f"{failed} battle scenario(s) failed; logs: {out}")


if __name__ == "__main__":
    main()
