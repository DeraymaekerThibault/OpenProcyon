"""Autoplay: run campaign missions unattended and summarise what broke (#68).
usage: python3 -m tools.autoplay [MISSION ...] [--speed N] [--limit SECONDS]   (default: missions 1-13, speed 8, limit 3600)
Each mission runs the release game with the `--script mission` autopilot steering the player's ship (PROCYON_AUTOPLAY in
game/src/smoke/mod.rs ends the run). Logs go to local/autoplay/missionNN.log; the table lists the outcome, the game time
reached, peak entity/particle counts, the slowest frame and how many rules never fired (listed in the log's last line).
Needs a display, or Xvfb + Mesa like tools/shot.sh. The effective speed is capped by the frame rate (0.1 game s per frame)."""

import argparse
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
GAME = REPO / "game/target/release/openprocyon"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
PROFILE = re.compile(r"profile .*frame_ms=([\d.]+) entities=(\d+) .*particles=(\d+)")
END = re.compile(r'autoplay end outcome="([^"]+)" time=(\d+) never_fired=\[(.*)\]')


def summarise(log, code):
    """One table row from a run's log text and exit code."""
    peak = [0.0, 0, 0]  # slowest frame (ms, 5 s average), entities, particles
    for m in PROFILE.finditer(log):
        peak = [max(p, type(p)(v)) for p, v in zip(peak, m.groups(), strict=True)]
    end = END.search(log)
    if "panicked at" in log:
        outcome = "PANIC"
    elif end:
        outcome = end[1]
    else:
        outcome = f"exit {code}"
    time = end[2] if end else "?"
    never = len(re.findall(r'"(?:[^"\\]|\\.)*"', end[3])) if end else "?"
    return outcome, time, peak, never


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("missions", nargs="*", type=int, default=list(range(1, 14)))
    ap.add_argument("--speed", type=float, default=8)
    ap.add_argument(
        "--limit",
        type=float,
        default=3600,
        help="game seconds before giving up on a mission",
    )
    args = ap.parse_args()
    subprocess.run(
        ["cargo", "build", "--release", "--quiet"], cwd=REPO / "game", check=True
    )
    out = REPO / "local/autoplay"
    out.mkdir(parents=True, exist_ok=True)
    wrap = (
        []
        if os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")
        else ["xvfb-run", "-a", "-s", "-screen 0 1280x720x24"]
    )
    rows = []
    with tempfile.TemporaryDirectory() as xdg:
        # A scratch XDG_DATA_HOME: default key bindings, master volume 0 (other options default) and no saves of the player's;
        # X11, since some Wayland sessions render black.
        (Path(xdg) / "openprocyon").mkdir()
        (Path(xdg) / "openprocyon/settings.json").write_text(json.dumps({"master": 0}))
        env = {
            **os.environ,
            "XDG_DATA_HOME": xdg,
            "WAYLAND_DISPLAY": "",
            "PROCYON_AUTOPLAY": str(args.limit),
            "PROCYON_PROFILE": "1",
        }
        if wrap:
            env["WGPU_BACKEND"] = "vulkan"
        for n in args.missions:
            log = out / f"mission{n:02}.log"
            cmd = [*wrap, str(GAME), "--mission", str(n), "--script", "mission", "--speed", str(args.speed),
                   "--shot", str(out / f"mission{n:02}.png"), "--shot-frame", "1000000000", "--late-frame", "0"]  # fmt: skip
            print(f"mission {n} ...", flush=True)
            with log.open("w") as f:
                try:
                    # Wall-clock cap for a hung run; below 10 fps the game runs slower than 1x, so leave room.
                    code = subprocess.run(
                        cmd,
                        cwd=REPO / "game",
                        env=env,
                        stdout=f,
                        stderr=subprocess.STDOUT,
                        timeout=args.limit * 1.5 + 120,
                    ).returncode
                except subprocess.TimeoutExpired:
                    code = "timeout"
            rows.append(
                (n, *summarise(ANSI.sub("", log.read_text(errors="replace")), code))
            )
    print(
        f"\n{'mission':>7}  {'outcome':<13} {'game s':>6}  {'worst ms':>8}  {'entities':>8}  {'particles':>9}  {'never fired':>11}"
    )
    for n, outcome, time, (ms, ents, parts), never in rows:
        print(
            f"{n:>7}  {outcome:<13} {time:>6}  {ms:>8.1f}  {ents:>8}  {parts:>9}  {never:>11}"
        )
    print(f"\nlogs: {out}")


if __name__ == "__main__":
    main()
