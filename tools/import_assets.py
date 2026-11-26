"""Import: convert the original install into the game's asset folder. Nothing from the install is modified.
usage: python -m tools.import_assets [--game DIR] [--out DIR] [Ship_Name ...]   (default ships below; first is the default player ship)
       openprocyon-import [--game DIR] [--out DIR] [Ship_Name ...]   (the same, frozen into the release download)
       openprocyon-import --bake-skies [options]   (the optional HD skies: tools.bake_sky on the imported assets)
       openprocyon-import --bake-surfaces [options]   (the optional surface maps: tools.bake_surfaces)
  --game  the original install (default: PROCYON_GAME_ROOT from the environment or .env)
  --out   where the converted assets go (default: assets/ in this checkout, or for the frozen importer the per-user folder
          ${XDG_DATA_HOME:-~/.local/share}/openprocyon/assets or %APPDATA%\\openprocyon\\assets; the game finds all of these,
          see base_assets() in game/src/lib.rs)
An existing --out is replaced whole: the import is built in <out>.importing and swapped in only when every step succeeded.
Each step prints `[n/total] name` first, so a front end can show progress. Exits non-zero on the first failing step."""

import contextlib
import io
import json
import multiprocessing
import os
import runpy
import shutil
import subprocess
import sys
import time
import traceback
import warnings
from pathlib import Path

from tools import importlog
from tools.gamefs import ci_files
from tools.jsonio import dump
from tools.skirmish_ships import SKIRMISH_SHIPS

REPO = Path(__file__).resolve().parents[1]
# The release download runs this module frozen by PyInstaller (no checkout, no Python install).
FROZEN = getattr(sys, "frozen", False)
# Every Ship_*.wot is exported; the curated retail skirmish ships come first (first = default player ship). Hulls that are not combat
# ships (wrecks, debris) fail the exporter and are skipped.
FIRST = SKIRMISH_SHIPS
SCENERY = ["Island01", "Island02", "IslandSmall01", "IslandMed01", "Islandrocky1"]
# Music: one skirmish track (Red Rover's script plays BTL_NavyBig_Full02); the other maps use other tracks.
MUSIC = ["BTL_NavyBig_Full02.ogg", "FrontEndMusic.ogg"]


# Layout version of the files an import writes, recorded in <assets>/importer.json (the game does not read it; tools and mods can).
# Raise it when a file's meaning or location changes. JSON is written by tools.jsonio: indent 2, number lists on one line.
FORMAT = 1


class StepFailed(Exception):
    pass


def tool(module, *args, quiet=False):
    """Run `python -m <module> <args>` in this process (the steps also run from the frozen importer, which has no Python to
    start); with quiet, return (ok, last error line) instead of failing."""
    argv, sys.argv = sys.argv, [module, *map(str, args)]
    err = io.StringIO()
    try:
        with contextlib.ExitStack() as stack:
            if quiet:
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                stack.enter_context(contextlib.redirect_stderr(err))
            with warnings.catch_warnings():
                # The step modules are also imported as libraries by each other; re-running one as __main__ is intended.
                warnings.filterwarnings(
                    "ignore", ".*found in sys.modules", RuntimeWarning
                )
                try:
                    runpy.run_module(module, run_name="__main__", alter_sys=True)
                except SystemExit as e:
                    if e.code not in (None, 0):
                        print(
                            e.code if isinstance(e.code, str) else f"exit {e.code}",
                            file=sys.stderr,
                        )
                        raise StepFailed(module) from None
                except StepFailed:
                    raise
                except Exception:
                    traceback.print_exc()
                    raise StepFailed(module) from None
    except StepFailed:
        if not quiet:
            raise
        return False, (err.getvalue().strip().splitlines() or [""])[-1]
    finally:
        sys.argv = argv
    return True, ""


def user_dir(env=os.environ, windows=sys.platform == "win32"):
    """The per-user folder, chosen like campaign::user_dir() in the game: XDG_DATA_HOME, then HOME/.local/share and
    APPDATA; on Windows APPDATA first (shells such as Git Bash set HOME), unless only a HOME one exists already."""
    if env.get("XDG_DATA_HOME"):
        return Path(env["XDG_DATA_HOME"], "openprocyon")
    home = env.get("HOME") and Path(env["HOME"], ".local/share/openprocyon")
    appdata = env.get("APPDATA") and Path(env["APPDATA"], "openprocyon")
    if windows and appdata and not (home and home.is_dir() and not appdata.is_dir()):
        return appdata
    return home or appdata or None


def user_assets():
    """The per-user asset folder the game reads."""
    d = user_dir()
    if not d:
        sys.exit("no XDG_DATA_HOME, HOME or APPDATA to put the assets in: pass --out")
    return d / "assets"


def main_worktree(repo=REPO):
    """The main checkout when `repo` is a linked git worktree (it shares the main checkout's git dir), else `repo` itself."""
    try:
        common = subprocess.run(
            [
                "git",
                "-C",
                str(repo),
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
            ],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return repo
    return Path(common).parent


def find_assets(env=os.environ, repo=REPO):
    """Where the converted assets are; the one lookup order of every tool (tests, check.sh, shot.sh, smokes, battle_check,
    autoplay): PROCYON_ASSETS, assets/ in this checkout, assets/ in the main checkout (a linked worktree uses the main
    import, never a copy), then the per-user folder. None when no import is found. The tools export the result as
    PROCYON_ASSETS so the game reads the same folder (its own order, base_assets() in game/src/lib.rs, has no worktree step)."""
    if env.get("PROCYON_ASSETS"):
        return Path(env["PROCYON_ASSETS"])
    user = user_dir(env)
    for d in (
        repo / "assets",
        main_worktree(repo) / "assets",
        user and user / "assets",
    ):
        if d and (d / "ships").is_dir():
            return d
    return None


def read_env(path):
    """KEY=value lines of a .env file (optional quotes, # comments)."""
    env = {}
    if path.is_file():
        for line in path.read_text(encoding="utf-8").splitlines():
            key, sep, value = line.strip().partition("=")
            if sep and not key.startswith("#"):
                env[key.strip()] = value.strip().strip("\"'")
    return env


# Rough share of a full import's time per step (seconds on a desktop; the rest take about one), so the overall percentage moves
# with time rather than with the step count. Only for the progress display.
WEIGHTS = {"ships": 31, "maps": 10, "campaign": 9, "collision": 8, "alpha bleed": 6}


class Progress:
    """Prints `[n/total] name (p%)` before each step and `[n/total] name: i/k (p%)` inside steps that report parts; p is the
    whole import's progress, weighted by WEIGHTS."""

    def __init__(self):
        self.names, self.n = [], 0

    def percent(self, fraction=0.0):
        w = [WEIGHTS.get(name, 1) for name in self.names]
        done = sum(w[: self.n - 1]) + w[self.n - 1] * fraction
        return int(100 * done / sum(w))

    def step(self, n):
        self.n = n
        print(
            f"[{n}/{len(self.names)}] {self.names[n - 1]} ({self.percent()}%)",
            flush=True,
        )

    def part(self, i, k):
        name = self.names[self.n - 1]
        print(
            f"[{self.n}/{len(self.names)}] {name}: {i + 1}/{k} ({self.percent(i / k)}%)",
            flush=True,
        )


def export_ships(root, out, want, part=lambda i, k: None):
    ships = []
    for i, s in enumerate(want):
        part(i, len(want))
        ok, err = tool("tools.export_combat", root, s, out / f"{s}.json", quiet=True)
        if not ok:
            (out / f"{s}.json").unlink(missing_ok=True)
            # wrecks/debris are not combat ships; anything else is an exporter bug
            print(f"skip {s}: {err}")
            importlog.skipped(f"WorldObjects/{s}.wot", err)
            continue
        scene = json.loads((out / f"{s}.json").read_text(encoding="utf-8"))["scene"]
        glb = out / "models/ships" / f"{scene}.glb"
        if (
            not glb.is_file()
            and not tool("tools.mdb_to_glb", root, scene, glb, "--offsets", quiet=True)[
                0
            ]
        ):
            (out / f"{s}.json").unlink(missing_ok=True)
            print(f"skip {s} (no mesh)")
            importlog.skipped(f"WorldObjects/{s}.wot", f"no mesh for scene {scene}")
            continue
        ships.append(s)
    (out / "ships.json").write_text(dump(ships), encoding="utf-8")


def model(root, scene, glb):
    """One scenery/UI mesh; a mesh a modded install lacks is logged and skipped, not fatal."""
    if glb.is_file():
        return
    ok, err = tool("tools.mdb_to_glb", root, scene, glb, quiet=True)
    if not ok:
        importlog.skipped(f"BD_MeshScenes/{scene}.msb", err)


def models(root, out):
    for kind, scenes in (("sky", ["skybox_07"]), ("scenery", SCENERY)):
        for scene in scenes:
            model(root, scene, out / "models" / kind / f"{scene}.glb")
    for part in ("COMPASS", "ARROW"):
        name = f"SELECTION_INDICATOR_{part}_SMALL"
        model(root, name, out / "models/ui" / f"{name}.glb")
    # The lob-aim ground marker: one mesh per weapon, and a "no firing solution" twin of each.
    for weapon in ("GRAVCHARGE", "STARMORTAR", "NOVAMORTAR"):
        for suffix in ("", "_HAS_NO_FIRING_SOLUTION"):
            name = f"LOBTARGET_{weapon}{suffix}"
            model(root, name, out / "models/ui" / f"{name}.glb")


def sounds(root, out):
    """Original Ogg Vorbis, copied with lowercase names (WOT names differ in case from the files)."""
    (out / "sound").mkdir(exist_ok=True)
    files = ci_files(root / "Sound/SoundEffects", "*.ogg") + ci_files(
        root / "Sound/GUI", "*.ogg"
    )
    for m in MUSIC:
        found = ci_files(root / "Sound/Music", m)
        if found:
            files += found
        else:
            importlog.skipped(f"Sound/Music/{m}", "file not found")
    for f in files:
        if not (out / "sound" / f.name.lower()).is_file():
            shutil.copyfile(f, out / "sound" / f.name.lower())


def flags(root, out):
    """Ship flags, converted to PNG."""
    from PIL import Image

    (out / "flags").mkdir(exist_ok=True)
    for f in ci_files(root / "BD_FlagTextures", "*.tga"):
        Image.open(f).convert("RGBA").save(out / "flags" / (f.stem.lower() + ".png"))


def steps(root, out, want, part=lambda i, k: None):
    def ship_files():
        names = json.loads((out / "ships.json").read_text(encoding="utf-8"))
        return [out / f"{s}.json" for s in names]

    return [
        ("ships", lambda: export_ships(root, out, want, part)),
        ("models", lambda: models(root, out)),
        ("hud", lambda: tool("tools.export_hud", root, out)),
        # Waypoint/course art lives in BD_Textures (DDS), not BD_GUITextures.
        ("waypoint art", lambda: tool("tools.export_waypoint_art", root, out)),
        ("slices", lambda: tool("tools.make_slices", out)),
        ("main bar", lambda: tool("tools.compose_bar", out)),
        # Font sheets, waypoint art and the slice strips (the sprites are fixed as they are exported).
        ("alpha bleed", lambda: tool("tools.fix_alpha_bleed", out / "hud/tex")),
        ("sounds", lambda: sounds(root, out)),
        ("flags", lambda: flags(root, out)),
        # Maps (World/*.twt), their scenery/skybox GLBs, music, backdrops; front-end screen layouts (Screens/*.tst) and strings.
        ("maps", lambda: tool("tools.export_maps", root, out)),
        ("campaign", lambda: tool("tools.export_campaign", root, out)),
        # HUD texture overlays (credits, tutorial pages).
        ("overlays", lambda: tool("tools.export_overlays", root, out)),
        # Hull/sail damage decals.
        ("decals", lambda: tool("tools.export_decals", root, out)),
        # Nebula/storm setups into the maps (after campaign).
        ("nebulae", lambda: tool("tools.export_nebulae", root, out)),
        # Mission-start saves (counters from retail SavedGames).
        ("saves", lambda: tool("tools.export_saves", root, out)),
        ("docks", lambda: tool("tools.export_docks", root, out)),
        ("mesh detail", lambda: tool("tools.export_mesh_detail", root, out)),
        ("mesh effects", lambda: tool("tools.export_mesh_effects", root, out)),
        ("animations", lambda: tool("tools.export_animations", root, out)),
        ("island sounds", lambda: tool("tools.export_island_sounds", root, out)),
        ("boarding", lambda: tool("tools.export_boarding", root, out)),
        ("asteroids", lambda: tool("tools.export_asteroids", root, out)),
        ("campaign media", lambda: tool("tools.export_campaign_media", root, out)),
        ("crew", lambda: tool("tools.export_crew", root, out)),
        # Crew lines named by content/voice/events.json.
        ("voices", lambda: tool("tools.export_voices", "copy", root, out)),
        # Campaign may append classes when a custom ship list was supplied.
        ("effects", lambda: tool("tools.export_effects", root, out, *ship_files())),
        # Split the raw per-ship exports into the editable data/ layout (docs/data-format.md).
        ("ship data", lambda: tool("tools.pack_data", out)),
        ("ship names", lambda: tool("tools.export_ship_names", root, out)),
        ("collision", lambda: tool("tools.export_collision", root, out)),
        ("format", lambda: tool("tools.format_json", out)),
        # Regroup into one folder per ship (out/ships/<Id>/) and per map/mission (out/maps/<id>/).
        ("layout", lambda: tool("tools.pack_layout", out)),
        # Armed bases (island gun towers) join the ship folders; needs data/weapons from pack_data.
        ("bases", lambda: tool("tools.export_bases", root, out)),
        # The base guns' bullet, impact and muzzle effects (no stock ship carries them).
        ("base effects", lambda: tool("tools.export_effects", root, out)),
    ]


def main(argv):
    if argv[:1] == ["--bake-skies"]:
        # The baker ships inside the frozen importer: the release download has no Python to run it with.
        from tools import bake_sky

        return bake_sky.main(argv[1:])
    if argv[:1] == ["--bake-surfaces"]:
        from tools import bake_surfaces

        return bake_surfaces.main(argv[1:])
    opts, want = {"--game": None, "--out": None}, []
    args = list(argv)
    while args:
        a = args.pop(0)
        if a in opts and args:
            opts[a] = args.pop(0)
        elif a.startswith("-"):
            sys.exit(__doc__)
        else:
            want.append(a)
    game, out = opts["--game"], opts["--out"]
    game = (
        game
        or os.environ.get("PROCYON_GAME_ROOT")
        or (not FROZEN and read_env(REPO / ".env").get("PROCYON_GAME_ROOT"))
    )
    if not game:
        sys.exit("pass --game <install dir> or set PROCYON_GAME_ROOT")
    root = Path(game).resolve()
    out = Path(out) if out else user_assets() if FROZEN else REPO / "assets"
    out = out.resolve()
    if not (root / "WorldObjects").is_dir():
        sys.exit(f"not a game install (no WorldObjects/): {root}")
    if not want:
        names = sorted(f.stem for f in ci_files(root / "WorldObjects", "Ship_*.wot"))
        want = FIRST + [n for n in names if n not in FIRST]
    # Every import is a clean pass: the steps skip files that already exist (models shared by many maps), so importing over an
    # earlier import would keep its outdated files. A folder with content is rebuilt beside it and replaced only on success.
    target = staging(out) if out.exists() and any(out.iterdir()) else out
    if target != out and target.exists():
        shutil.rmtree(target)  # an earlier import that failed or was stopped
    for kind in ("ships", "sky", "scenery", "ui"):
        (target / "models" / kind).mkdir(parents=True, exist_ok=True)
    log = (
        (user_dir() or REPO / "local") / "import.log"
        if FROZEN
        else REPO / "local/import.log"
    )
    importlog.start(log)
    progress = Progress()
    todo = steps(root, target, want, progress.part)
    progress.names = [name for name, _ in todo]
    for n, (name, run) in enumerate(todo, 1):
        progress.step(n)
        try:
            run()
        except StepFailed as e:
            sys.exit(f"import failed at step {name!r}: {e}")
    (target / "importer.json").write_text(dump(dict(format=FORMAT)), encoding="utf-8")
    if target != out:
        try:
            swap_in(target, out)
        except OSError as e:
            sys.exit(
                f"the import finished in {target}, but it could not replace {out} ({e}); close programs using it and retry"
            )
    print(importlog.summary(len(todo)))
    print(f"done: {out}")


def staging(out):
    """Where an import over an existing folder is written first (the name the game's setup screen also uses)."""
    return out.with_name(f"{out.name}.importing")


def swap_in(staged, out):
    """Puts the finished import `staged` in place of `out`; the old folder is removed only once the new one is in place."""
    old = out.with_name(f"{out.name}.replaced")
    if old.exists():
        shutil.rmtree(old)
    retry(lambda: out.rename(old))
    try:
        retry(lambda: staged.rename(out))
    except OSError:
        old.rename(out)
        raise
    shutil.rmtree(old, ignore_errors=True)


def retry(f, tries=20):
    """Windows refuses to rename a folder while a virus scanner or the indexer briefly holds a file in it."""
    for i in range(tries):
        try:
            return f()
        except OSError:
            if i == tries - 1:
                raise
            time.sleep(0.25)


if __name__ == "__main__":
    # The baker's worker processes start this program again; frozen, they have to be caught here, before main() (a no-op
    # when not frozen).
    multiprocessing.freeze_support()
    main(sys.argv[1:])
