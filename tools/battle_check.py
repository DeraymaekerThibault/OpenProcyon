"""Battle check: play short skirmish battles and fail when one stops behaving like a battle.
usage: python3 -m tools.battle_check [NAME ...] [--speed N]   (default: every scenario below)
Two kinds of scenario: a map with the --ship/--allies/--enemies fleet shortcut, and a lobby scenario (`--lobby <spec>`,
game/src/menu/lobby_scenario.rs) that sets the skirmish lobby up like a player (slots, victory points, fleet limit) and presses
Launch. The shortcut never plays an allied computer slot or a full lobby; only lobby scenarios do.
Each scenario runs the release game with PROCYON_REPORT (game/src/smoke/report.rs), which logs one `battle report {json}` line
when one side is gone or the time is up. The expectations below are whole-battle invariants (the computer engages, nobody grinds on
an island, nothing panics): they catch regressions where one feature quietly changes another (armed volcanoes taken for bases,
map-script tags hiding the captain's target). Runs take ~10-40 s each; logs go to local/battle_check/<name>.log.
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
from typing import NamedTuple

REPO = Path(__file__).resolve().parents[1]
GAME = (
    Path(os.environ.get("CARGO_TARGET_DIR", REPO / "game/target"))
    / "release/openprocyon"
)
ANSI = re.compile(r"\x1b\[[0-9;]*m")
REPORT = re.compile(r"battle report (\{.*\})")
FLEETS = ["--ship", "Ship_Navy_Cutter", "--allies", "Ship_Navy_Cutter",
          "--enemies", "Ship_Pirate_Barque,Ship_Pirate_Sloop"]  # fmt: skip
# Seconds a ship may stay in contact with scenery without a break before it counts as grinding. A wedged computer ship first waits
# for the original's unstick (the same obstacle for 5.5 s), then turns in place until it is free (docs/re/movement.md).
GRIND = 10.0
# Frames over 50 ms a battle may have (after the first second) before it counts as stuttering; healthy runs have 0-1.
STUTTER = 10

# Share of a victory-point budget a computer fleet should spend. PROVISIONAL: the retail figure is being measured.
SPEND = 0.6
# Seconds in which every side should have fired. PROVISIONAL: not measured on the retail maps; tighten after the first real runs.
FIRE_BY = 240


class Scenario(NamedTuple):
    name: str
    args: list  # game command line (without --speed)
    seconds: int  # game seconds to watch
    expect: set


def shortcut(name, map_id, seconds, expect):
    """A map with the fixed --ship/--allies/--enemies fleets (the human's own fleet; no lobby)."""
    return Scenario(name, ["--map", map_id, *FLEETS], seconds, expect)


def lobby(name, spec, seconds, expect):
    """A skirmish lobby setup played through the lobby's own model and Launch path."""
    return Scenario(name, ["--lobby", spec], seconds, expect)


# Expectations. "fight": both sides fire and hit, the computer's first burst within 60 s. "patrol": every computer-commanded ship
# (allied computer fleets included, the human's own fleet excluded) that is still afloat sails at least 300 units. "fire": every side fires within FIRE_BY
# s, and on a side with computer ships one of them fires. "budget": every computer fleet spends most of its victory points.
# Every scenario also checks for panics, grinding and stutter ("no-stutter" skips the last, only with an open issue).
SCENARIOS = [
    shortcut("ambush", "ambush", 120, {"fight"}),  # starts 514 apart among 22 islands
    shortcut("bayles_stand", "bayles_stand", 120, {"fight"}),  # starts 464 apart
    shortcut(
        "diablo_strait", "diablo_straight", 120, {"patrol"}
    ),  # armed volcanoes, starts 861 apart
    shortcut("border_dispute", "border_dispute", 120, {"patrol"}),  # armed bases
    # The owner's game: Border Dispute, 1000 points, 10 ships, every other slot a computer. Allied computers share the human's
    # side; before they hunted like any computer they sat still (the shortcut above cannot see that).
    # PROVISIONAL: "budget" is left out until computer fleets at large budgets are fixed (it would fail today); it is reported anyway.
    lobby(
        "border_dispute_full",
        "map=border_dispute;points=1000;limit=10;slots=human,7*computer",
        300,
        {"patrol", "fire", "budget"},
    ),
]


def computer(ship):
    """A computer captain commands the ship. Older reports only had the side: everything off the player's side."""
    return ship.get("computer", ship["side"] != "player")


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
    if "fire" in expect:
        for name, line in sides.items():
            first = line.get("first_burst")
            if first is None:
                fails.append(f"{name} never fired")
            elif first > FIRE_BY:
                fails.append(f"{name} first fired at {first:.0f} s")
            elif not any(
                s.get("bursts")
                for s in report["ships"]
                if s["side"] == name and computer(s)
            ) and any(computer(s) for s in report["ships"] if s["side"] == name):
                fails.append(f"no computer ship of {name} fired")
    if "patrol" in expect:
        fails += [
            f"{s['class']} ({s['side']}) sailed only {s['sailed']:.0f} units"
            for s in report["ships"]
            if computer(s) and not s.get("sunk") and s["sailed"] < 300
        ]
    if "budget" in expect:
        for f in report.get("fleets", []):
            if not f["human"] and f["spent"] < SPEND * f["budget"]:
                fails.append(
                    f"{f['side']} computer fleet (player {f['player']}) spent {f['spent']} of {f['budget']} points"
                )
    frames = report.get("frame_ms", {})
    if "no-stutter" not in expect and frames.get("over_50", 0) > STUTTER:
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
    fleets = [f for f in report.get("fleets", []) if not f["human"]]
    if fleets:
        sides += f"; computer fleets spent {sum(f['spent'] for f in fleets)}/{sum(f['budget'] for f in fleets)} points"
    return not fails, "; ".join(fails) or f"ok at {report['time']:.0f} s ({sides})"


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("names", nargs="*", help="scenarios to run (default: all)")
    ap.add_argument("--speed", type=float, default=16)
    args = ap.parse_args()
    chosen = [s for s in SCENARIOS if not args.names or s.name in args.names]
    if not chosen:
        sys.exit(
            f"no scenario named {args.names}; known: {[s.name for s in SCENARIOS]}"
        )
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
        for name, game_args, seconds, expect in chosen:
            log = out / f"{name}.log"
            env["PROCYON_REPORT"] = str(seconds)
            cmd = [*wrap, str(GAME), *game_args, "--speed", str(args.speed)]
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
