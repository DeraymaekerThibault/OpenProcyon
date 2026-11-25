"""Battle check: play short skirmish battles and fail when one stops behaving like a battle.
usage: python3 -m tools.battle_check [NAME ...] [--speed N] [--profile fast|release] [--jobs N] [--twice | --replay]   (default: every scenario below)
Two kinds of scenario: a map with the --ship/--allies/--enemies fleet shortcut, and a lobby scenario (`--lobby <spec>`,
game/src/menu/lobby_scenario.rs) that sets the skirmish lobby up like a player (slots, victory points, fleet limit) and presses
Launch. The shortcut never plays an allied computer slot or a full lobby; only lobby scenarios do.
Each scenario runs the game (Cargo profile `fast` unless --profile) with PROCYON_REPORT (game/src/smoke/report.rs), which logs
one `battle report {json}` line when one side is gone or the time is up. The expectations below are whole-battle invariants (the computer engages, nobody grinds on
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
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import NamedTuple

from tools import gamelog
from tools.autoplay import assets_env, build_game, display_wrapper, silent_data_home

REPO = Path(__file__).resolve().parents[1]
REPORT = re.compile(r"battle report (\{.*\})")
FLEETS = ["--ship", "Ship_Navy_Cutter", "--allies", "Ship_Navy_Cutter",
          "--enemies", "Ship_Pirate_Barque,Ship_Pirate_Sloop"]  # fmt: skip
# Seconds a ship may stay in contact with scenery without a break before it counts as grinding. A wedged computer ship first waits
# for the original's unstick (the same obstacle for 5.5 s), then turns in place until it is free (docs/re/movement.md).
GRIND = 10.0
# Frames over 50 ms a battle may have (after the first second) before it counts as stuttering; healthy runs have 0-1.
STUTTER = 10

# Whole-run sanity limits, checked in every scenario. PROVISIONAL: generous until real runs show the healthy ranges.
# Furthest any ship may get from the origin along one axis (game units) and the most entities a battle may hold at once.
BOUNDS = 50000
ENTITY_CAP = 50000

MIXED = ["--ship", "Ship_Navy_Frigate", "--allies", "Ship_Navy_Cutter,Ship_Navy_Frigate",
         "--enemies", "Ship_Pirate_Barque,Ship_Pirate_Sloop,Ship_Pirate_Barque"]  # fmt: skip

# Share of a victory-point budget a computer fleet should spend. PROVISIONAL: the retail figure is being measured.
SPEND = 0.6
# Seconds in which every side should have fired. PROVISIONAL: not measured on the retail maps; tighten after the first real runs.
FIRE_BY = 240


class Scenario(NamedTuple):
    name: str
    args: list  # game command line (without --speed)
    seconds: int  # game seconds to watch
    expect: set


def shortcut(name, map_id, seconds, expect, fleets=FLEETS):
    """A map with the fixed --ship/--allies/--enemies fleets (the human's own fleet; no lobby)."""
    return Scenario(name, ["--map", map_id, *fleets], seconds, expect)


def lobby(name, spec, seconds, expect):
    """A skirmish lobby setup played through the lobby's own model and Launch path."""
    return Scenario(name, ["--lobby", spec], seconds, expect)


# Expectations. "fight": both sides fire and hit, the computer's first burst within 60 s. "engage": the same for side1 only
# (the human fleet receives no orders, so its firing is not guaranteed). "patrol": every computer-commanded ship
# (allied computer fleets included, the human's own fleet excluded) that is still afloat sails at least 300 units. "fire": every side fires within FIRE_BY
# s, and on a side with computer ships one of them fires. "budget": every computer fleet spends most of its victory points.
# "resolve": the battle ends (one side left) or a ship sinks within the time. PROVISIONAL.
# Every scenario also checks for panics, grinding, non-finite or far-off positions, runaway entity counts and stutter ("no-stutter" skips the last, only with an open issue).
SCENARIOS = [
    shortcut("ambush", "ambush", 120, {"engage"}),  # starts 514 apart among 22 islands
    shortcut("bayles_stand", "bayles_stand", 120, {"engage"}),  # starts 464 apart
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
        {"patrol", "fire", "budget", "resolve"},
    ),
    # More maps and mixed fleets (only the whole-run invariants and the computer's engagement are judged). PROVISIONAL: red_rover and the
    # mixed fleets have not been watched for long; loosen "engage" if the first real runs say so.
    shortcut("red_rover", "red_rover", 120, {"engage"}),
    shortcut("mixed_fleets", "red_rover", 120, set(), fleets=MIXED),
]


def computer(ship):
    """A computer captain commands the ship. Older reports only had the side: everything off the player's side."""
    return ship.get("computer", ship["side"] != "player")


def sanity(s):
    """The failed whole-run invariants of a report's `sanity` block (absent in older reports: nothing to judge)."""
    fails = []
    if s.get("non_finite", 0):
        fails.append(
            f"{s['non_finite']} ship positions were not finite (NaN or infinity)"
        )
    if s.get("farthest", 0) > BOUNDS:
        fails.append(
            f"a ship got {s['farthest']:.0f} units from the origin (limit {BOUNDS})"
        )
    if s.get("entities", 0) > ENTITY_CAP:
        fails.append(f"{s['entities']} entities alive at once (limit {ENTITY_CAP})")
    return fails


def comparable(report):
    """A report without what differs between runs of one seed: the real frame times."""
    return {k: v for k, v in report.items() if k != "frame_ms"}


def replayable(report):
    """A report without what a replay does not reproduce: frame times, and the entity peak, which counts presentation (sounds end
    with their playback, not on a tick)."""
    out = comparable(report)
    out["sanity"] = {k: v for k, v in out.get("sanity", {}).items() if k != "entities"}
    return out


def same_run(log_a, log_b, keep=comparable, what="same report twice"):
    """(same, why) for two logs of one seeded scenario (two fixed-step runs, or a run and its replay): the battle reports must match."""
    reports = [REPORT.search(log) for log in (log_a, log_b)]
    if not all(reports):
        return False, "a repeat produced no battle report"
    a, b = (keep(json.loads(m[1])) for m in reports)
    if a == b:
        return True, what
    keys = sorted(k for k in a.keys() | b.keys() if a.get(k) != b.get(k))
    return False, f"the same seed gave different reports ({', '.join(keys)})"


def evaluate(report, expect):
    """The failed expectations of one battle report (empty when it passes)."""
    fails = []
    sides = report["sides"]
    if "fight" in expect or "engage" in expect:
        for name in ("player", "side1") if "fight" in expect else ("side1",):
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
    if (
        "resolve" in expect
        and not report.get("ended", True)
        and not any(s.get("sunk") for s in report["ships"])
    ):
        fails.append(f"nothing sank and no side was beaten in {report['time']:.0f} s")
    fails += sanity(report.get("sanity", {}))
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


def deterministic(on):
    """(extra game arguments, extra environment) of a reproducible run: fixed step and the seed 1."""
    return (["--fixed-step", "--seed", "1"], {}) if on else ([], {})


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("names", nargs="*", help="scenarios to run (default: all)")
    ap.add_argument("--speed", type=float, default=16)
    ap.add_argument(
        "--profile",
        default="fast",
        help="Cargo profile to build the game with (default fast; release to time what players get)",
    )
    ap.add_argument(
        "--jobs",
        type=int,
        default=0,
        help="scenarios to play at once, each with its own display (default: half the cores; 1 to judge stutter)",
    )
    ap.add_argument(
        "--twice",
        action="store_true",
        help="play each scenario twice, fixed-step with seed 1, and fail when the two reports differ",
    )
    ap.add_argument(
        "--replay",
        action="store_true",
        help="play each scenario on the normal clock with `--script commands` recording a command log (PROCYON_COMMANDS), "
        "replay the log (`--replay`), and fail when the two reports differ",
    )
    args = ap.parse_args()
    chosen = [s for s in SCENARIOS if not args.names or s.name in args.names]
    if not chosen:
        sys.exit(
            f"no scenario named {args.names}; known: {[s.name for s in SCENARIOS]}"
        )
    game = build_game(args.profile)
    out = REPO / "local/battle_check"
    out.mkdir(parents=True, exist_ok=True)
    wrap = display_wrapper()
    # Software Vulkan is CPU-bound: half the cores is the most that pays off.
    jobs = max(1, min(args.jobs or (os.cpu_count() or 2) // 2, len(chosen)))
    if jobs > 1:
        print(
            f"battle_check: {jobs} jobs; stutter is not judged (load-sensitive: use --jobs 1 for that)"
        )

    def run(scenario):
        """Plays one scenario in its own display and scratch XDG_DATA_HOME; returns (passed, line)."""
        name, game_args, seconds, expect = scenario
        if jobs > 1 or args.twice or args.replay:
            expect = expect | {"no-stutter"}
        start = time.monotonic()
        log = out / f"{name}.log"
        extra, env_extra = deterministic(args.twice)
        if args.replay:
            extra, env_extra = (
                ["--script", "commands"],
                {"PROCYON_COMMANDS": str(log.with_suffix(".commands.jsonl"))},
            )
        text = play(name, game_args, seconds, log, extra, env_extra)
        passed, summary = outcome(text, expect)
        if passed and args.replay:
            passed, summary = replay(name, game_args, seconds, text, log)
        if passed and args.twice:
            passed, summary = same_run(
                text,
                play(
                    name,
                    game_args,
                    seconds,
                    out / f"{name}.repeat.log",
                    *deterministic(True),
                ),
            )
        line = f"{'ok  ' if passed else 'FAIL'} {name:<16} {time.monotonic() - start:3.0f}s {summary}"
        if not passed:
            line += f"\n    rerun: python3 -m tools.battle_check {name} --jobs 1   (log: {log})"
            line += "".join(
                "\n    " + ln for ln in text.splitlines()[-10:] if ln.strip()
            )
        return passed, line

    def replay(name, game_args, seconds, text, log):
        """(same, why) for the recorded run `text` (log `log`) and a replay of its command log."""
        commands = log.with_suffix(".commands.jsonl")
        if not commands.exists():
            return False, f"the run wrote no command log ({commands})"
        again = play(
            name,
            game_args,
            seconds,
            out / f"{name}.replay.log",
            ["--replay", str(commands)],
            {},
        )
        return same_run(text, again, replayable, "the replay gave the same report")

    def play(name, game_args, seconds, log, extra, env_extra):
        """Plays one scenario in its own display and scratch XDG_DATA_HOME; returns the log text."""
        with tempfile.TemporaryDirectory() as xdg:
            # A scratch XDG_DATA_HOME (default bindings, no saves, silent) and X11, as tools/autoplay.py.
            silent_data_home(xdg)
            env = {
                **os.environ,
                "XDG_DATA_HOME": xdg,
                **assets_env(),
                "WAYLAND_DISPLAY": "",
                "PROCYON_OVERLAY": str(Path(xdg) / "overlay"),
                "PROCYON_REPORT": str(seconds),
                **env_extra,
            }
            if wrap:
                env["WGPU_BACKEND"] = "vulkan"
            cmd = [*wrap, str(game), *game_args, "--speed", str(args.speed), *extra]
            # Wall-clock cap for a hung run (below ~10 fps the game runs slower than asked; a fixed-step run needs one frame per tick); a timeout reports no battle.
            with (
                log.open("w", encoding="utf-8") as f,
                contextlib.suppress(subprocess.TimeoutExpired),
            ):
                subprocess.run(
                    cmd,
                    cwd=REPO / "game",
                    env=env,
                    stdout=f,
                    stderr=subprocess.STDOUT,
                    timeout=seconds * (4 if extra else 1) + 120,
                )
        return gamelog.read(log, r"panicked at|battle report")

    start = time.monotonic()
    with ThreadPoolExecutor(jobs) as pool:
        # Results print in the order the runs finish.
        futures = [pool.submit(run, s) for s in chosen]
        results = []
        for f in as_completed(futures):
            results.append(f.result())
            print(results[-1][1], flush=True)
    failed = sum(not passed for passed, _ in results)
    print(
        f"battle_check: {len(results) - failed} ok, {failed} FAIL in {time.monotonic() - start:.0f}s (jobs {jobs})"
    )
    if failed:
        sys.exit(f"{failed} battle scenario(s) failed; logs: {out}")


if __name__ == "__main__":
    main()
