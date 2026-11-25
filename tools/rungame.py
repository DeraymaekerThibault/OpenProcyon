"""Running the game from a tool: build it, give it a display, a scratch data home and the imported assets, log its output.
Shared by tools.autoplay and tools.battle_check (the shell twin of this is tools/shot.sh)."""

import json
import os
import subprocess
import sys
from pathlib import Path

from tools.import_assets import find_assets
from tools.paths import REPO


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


def display_wrapper():
    """The command prefix that gives the game a display: none where there is one, else a virtual X server."""
    if (
        sys.platform == "win32"
        or os.environ.get("DISPLAY")
        or os.environ.get("WAYLAND_DISPLAY")
    ):
        return []
    # -d lets Xvfb pick a free display (-displayfd); -a races between parallel runs. Older xvfb-run (Debian) has only -a.
    helptext = subprocess.run(
        ["xvfb-run", "--help"], capture_output=True, text=True
    ).stdout
    auto = "-d" if "--auto-display" in helptext else "-a"
    return ["xvfb-run", auto, "-s", "-screen 0 1280x720x24"]


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


def game_env(xdg, wrap, **extra):
    """The environment of a tool-run game: the scratch XDG_DATA_HOME, the assets, X11 (some Wayland sessions render black), and
    Vulkan when `wrap` (the display_wrapper() prefix) means a virtual display. `extra` (PROCYON_* settings) wins over all of it."""
    env = {
        **os.environ,
        "XDG_DATA_HOME": str(xdg),
        **assets_env(),
        "WAYLAND_DISPLAY": "",
        **extra,
    }
    if wrap:
        env["WGPU_BACKEND"] = "vulkan"
    return env


def run_logged(cmd, log, env, timeout):
    """Runs the game from game/ with stdout and stderr in the file `log`; returns the exit code, or None when `timeout` (a
    wall-clock cap for a hung run) ran out and the run was killed."""
    with log.open("w", encoding="utf-8") as f:
        try:
            return subprocess.run(
                cmd,
                cwd=REPO / "game",
                env=env,
                stdout=f,
                stderr=subprocess.STDOUT,
                timeout=timeout,
            ).returncode
        except subprocess.TimeoutExpired:
            return None
