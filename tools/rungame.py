"""Running the game from a tool: build it, give it a display, a scratch data home and the imported assets, log its output.
Shared by tools.autoplay, tools.battle_check and tools/shot.sh (`python3 -m tools.rungame run -- <game> <args>`), and through
them by smokes.sh, visdiff.py and check.sh: the one place that decides how a game process is launched.

The run mode comes from PROCYON_RUN:
  window      the game in a window on the desktop session (needs DISPLAY on Linux: X11 or XWayland)
  software    a virtual X server (Xvfb) and Mesa's software Vulkan (lavapipe); no GPU, CPU-bound
  gpu-hidden  as window, and PROCYON_HIDDEN=1 asks the game to keep its window hidden
  auto        (or unset) window when there is a display or on Windows, else software
On Linux the game never talks to Wayland directly (WAYLAND_DISPLAY is cleared): some Wayland sessions render it black.
Every run prints one `run:` line with the mode, the graphics adapter and backend the game got, frames and wall time."""

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import NamedTuple

from tools import gamelog
from tools.import_assets import find_assets
from tools.paths import REPO

MODES = ("window", "software", "gpu-hidden")
# Bevy logs the wgpu adapter it got as `AdapterInfo { name: "...", ..., device_type: Cpu, ..., backend: Vulkan, ... }`, and the game
# logs `run end: frame N` when it stops (exit.rs).
ADAPTER = re.compile(
    r'AdapterInfo \{ name: "([^"]*)".*?device_type: (\w+).*?backend: (\w+)'
)
RUN_END = re.compile(r"run end: frame (\d+)")


class Launch(NamedTuple):
    """How to start one game process: the mode, the command prefix (a virtual display or nothing) and environment settings."""

    mode: str
    prefix: list
    env: dict


def windows(platform=sys.platform):
    """Windows, natively or from Git Bash/MSYS (shot.sh runs there): the game always gets the desktop."""
    return platform.startswith(("win", "msys", "cygwin"))


def run_mode(environ=os.environ, platform=sys.platform):
    """The mode PROCYON_RUN asks for, `auto` resolved; an unknown value is an error, not a silent fallback."""
    mode = environ.get("PROCYON_RUN", "") or "auto"
    if mode == "auto":
        display = environ.get("DISPLAY") or environ.get("WAYLAND_DISPLAY")
        return "window" if windows(platform) or display else "software"
    if mode not in MODES:
        raise ValueError(
            f"PROCYON_RUN={mode!r} is not a run mode; use one of {', '.join(MODES)} or auto"
        )
    return mode


def software_threads(jobs, cores=None):
    """Rendering threads for each of `jobs` software-rendered games at once, so jobs x threads stays within the cores."""
    cores = cores or os.cpu_count() or 2
    return max(1, cores // max(1, jobs))


def launch_plan(
    environ=os.environ, platform=sys.platform, jobs=1, xvfb_help=None, icds=None
):
    """The Launch for PROCYON_RUN in `environ` with `jobs` games running at once. `xvfb_help` (xvfb-run's --help text) and
    `icds` (Mesa's software Vulkan driver files) are looked up when not given."""
    mode = run_mode(environ, platform)
    win = windows(platform)
    # X11 (XWayland under a Wayland desktop), never native Wayland: some Wayland sessions render the game black.
    env = {} if win else {"WAYLAND_DISPLAY": ""}
    if mode == "software":
        if win:
            raise ValueError(
                "PROCYON_RUN=software needs Xvfb, which Windows does not have; use window"
            )
        if xvfb_help is None:
            xvfb_help = subprocess.run(
                ["xvfb-run", "--help"], capture_output=True, text=True
            ).stdout
        if icds is None:
            icds = sorted(Path("/usr/share/vulkan/icd.d").glob("lvp_icd.*.json"))
        # -d lets Xvfb pick a free display (-displayfd); -a races between parallel runs. Older xvfb-run (Debian) has only -a.
        auto = "-d" if "--auto-display" in xvfb_help else "-a"
        env["WGPU_BACKEND"] = "vulkan"
        # Mesa's software Vulkan when installed: on a machine with a GPU, Vulkan otherwise picks it and has no present mode on Xvfb.
        if icds:
            env["VK_ICD_FILENAMES"] = str(icds[0])
        # lavapipe rasterises with llvmpipe's threads, which LP_NUM_THREADS sets (default: one per core); a value the caller set wins.
        if jobs > 1 and "LP_NUM_THREADS" not in environ:
            env["LP_NUM_THREADS"] = str(software_threads(jobs))
        return Launch(mode, ["xvfb-run", auto, "-s", "-screen 0 1280x720x24"], env)
    if not win and not environ.get("DISPLAY"):
        raise ValueError(
            f"PROCYON_RUN={mode} needs an X11 display (DISPLAY; XWayland on a Wayland desktop); use software without one"
        )
    if mode == "gpu-hidden":
        env["PROCYON_HIDDEN"] = "1"
    return Launch(mode, [], env)


def summary(mode, log, seconds):
    """The one line a run prints: the mode, the adapter and backend the game got and its frame rate, from the game's log text."""
    adapter = ADAPTER.search(log)
    got = (
        f'adapter "{adapter[1]}" ({adapter[2]}, {adapter[3]})'
        if adapter
        else "adapter unknown (no AdapterInfo line)"
    )
    end = RUN_END.search(log)
    if end:
        frames = int(end[1])
        rate = (
            f"{frames} frames in {seconds:.1f} s ({frames / seconds:.1f} fps)"
            if seconds > 0
            else f"{frames} frames"
        )
    else:
        rate = f"{seconds:.1f} s (frame count unknown: no run end line)"
    return f"run: mode={mode} {got} {rate}"


def build_game(profile="fast"):
    """Builds the game with a Cargo profile (`fast` for checking runs, `release` as players get it) and returns the binary."""
    subprocess.run(
        ["cargo", "build", "--profile", profile, "--quiet"],
        cwd=REPO / "game",
        check=True,
    )
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "game/target"))
    # Cargo names the release profile's folder `release` and every custom profile's after the profile.
    return (
        target
        / profile
        / ("openprocyon.exe" if sys.platform == "win32" else "openprocyon")
    )


def assets_env():
    """PROCYON_ASSETS for a game run (the scratch XDG_DATA_HOME hides the per-user import and a linked worktree has none), and
    this checkout's own PROCYON_OVERLAY: the default overlay sits next to the assets, shared by every worktree using the main import."""
    assets = find_assets()
    overlay = os.environ.get("PROCYON_OVERLAY", str(REPO / "local/tool-overlay"))
    return {
        "PROCYON_OVERLAY": overlay,
        **({"PROCYON_ASSETS": str(assets)} if assets else {}),
    }


def silent_data_home(xdg):
    """Fills a scratch XDG_DATA_HOME with settings that mute the game (default bindings, no saves)."""
    (Path(xdg) / "openprocyon").mkdir()
    (Path(xdg) / "openprocyon/settings.json").write_text(
        json.dumps({"master": 0}), encoding="utf-8"
    )


def game_env(xdg, launch, **extra):
    """The environment of a tool-run game: the scratch XDG_DATA_HOME, the assets and the `launch` plan's settings (display,
    graphics). `extra` (PROCYON_* settings) wins over all of it."""
    return {
        **os.environ,
        "XDG_DATA_HOME": str(xdg),
        **assets_env(),
        **launch.env,
        **extra,
    }


def run_logged(cmd, log, env, timeout, mode=None):
    """Runs the game from game/ with stdout and stderr in the file `log`; returns the exit code, or None when `timeout` (a
    wall-clock cap for a hung run) ran out and the run was killed. With `mode` (the Launch's), prints the run's `run:` line."""
    start = time.monotonic()
    with log.open("w", encoding="utf-8") as f:
        try:
            code = subprocess.run(
                cmd,
                cwd=REPO / "game",
                env=env,
                stdout=f,
                stderr=subprocess.STDOUT,
                timeout=timeout,
            ).returncode
        except subprocess.TimeoutExpired:
            code = None
    if mode:
        text = gamelog.read(log, f"{ADAPTER.pattern}|{RUN_END.pattern}")
        print(
            f"{summary(mode, text, time.monotonic() - start)}  ({log.name})", flush=True
        )
    return code


def run(cmd, environ=os.environ, jobs=1):
    """Runs the game command `cmd` from game/ the way PROCYON_RUN says, its output passed through; prints the `run:` line after it and
    returns the exit code (tools/shot.sh). SIGTERM (a `timeout` around the caller) also stops the game."""
    plan = launch_plan(environ, jobs=jobs)
    start = time.monotonic()
    seen = []
    proc = subprocess.Popen(
        [*plan.prefix, *cmd],
        cwd=REPO / "game",
        env={**environ, **plan.env},
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    before = signal.signal(signal.SIGTERM, lambda *_: proc.terminate())
    keep = re.compile(f"{ADAPTER.pattern}|{RUN_END.pattern}")
    try:
        for raw in proc.stdout:
            sys.stdout.buffer.write(raw)
            sys.stdout.buffer.flush()
            line = gamelog.ANSI.sub("", raw.decode("utf-8", "replace"))
            if keep.search(line):
                seen.append(line)
        code = proc.wait()
    finally:
        signal.signal(signal.SIGTERM, before)
    sys.stdout.flush()
    print(summary(plan.mode, "".join(seen), time.monotonic() - start), flush=True)
    return code


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Run a game command in the PROCYON_RUN mode (see this module's docstring)."
    )
    sub = ap.add_subparsers(dest="what", required=True)
    r = sub.add_parser("run", help="run `-- <game> <args>` and print the run: line")
    r.add_argument("--jobs", type=int, default=int(os.environ.get("PROCYON_JOBS") or 1),
                   help="games running at once (default PROCYON_JOBS or 1): bounds software rendering threads")  # fmt: skip
    r.add_argument("cmd", nargs=argparse.REMAINDER)
    sub.add_parser("mode", help="print the mode PROCYON_RUN resolves to")
    args = ap.parse_args(argv)
    try:
        if args.what == "mode":
            print(launch_plan().mode)
            return 0
        cmd = args.cmd[1:] if args.cmd[:1] == ["--"] else args.cmd
        if not cmd:
            ap.error("run needs a command: run -- <game> <args>")
        return run(cmd, jobs=args.jobs)
    except ValueError as e:
        print(f"rungame: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
