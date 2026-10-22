"""Import: convert the original install into the game's asset folder. Nothing from the install is modified.
usage: python -m tools.import_assets [--game DIR] [--out DIR] [Ship_Name ...]   (default ships below; first is the default player ship)
       openprocyon-import [--game DIR] [--out DIR] [Ship_Name ...]   (the same, frozen into the release download)
  --game  the original install (default: PROCYON_GAME_ROOT from the environment or .env)
  --out   where the converted assets go (default: assets/ in this checkout, or for the frozen importer the per-user folder
          ${XDG_DATA_HOME:-~/.local/share}/openprocyon/assets or %APPDATA%\\openprocyon\\assets; the game finds all of these,
          see base_assets() in game/src/lib.rs)
Each step prints `[n/total] name` first, so a front end can show progress. Exits non-zero on the first failing step."""

import contextlib
import io
import json
import os
import runpy
import shutil
import sys
import traceback
import warnings
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
# The release download runs this module frozen by PyInstaller (no checkout, no Python install).
FROZEN = getattr(sys, "frozen", False)
# Every Ship_*.wot is exported; the curated retail skirmish ships come first (first = default player ship). Hulls that are not combat
# ships (wrecks, debris) fail the exporter and are skipped.
FIRST = [
    "Ship_Navy_Cutter",
    "Ship_Navy_Frigate",
    "Ship_Navy_FastFrigate",
    "Ship_Pirate_Schooner",
    "Ship_Pirate_Sloop",
    "Ship_Pirate_Gunboat",
    "Ship_Pirate_Barque",
]
SCENERY = ["Island01", "Island02", "IslandSmall01", "IslandMed01", "Islandrocky1"]
# Music: one skirmish track (Red Rover's script plays BTL_NavyBig_Full02); the other maps use other tracks.
MUSIC = ["BTL_NavyBig_Full02.ogg", "FrontEndMusic.ogg"]


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
                    "ignore", "found in sys.modules", RuntimeWarning
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


def user_assets():
    """The per-user asset folder, looked up in the same order as campaign::user_dir() in the game."""
    for var, sub in (("XDG_DATA_HOME", ""), ("HOME", ".local/share"), ("APPDATA", "")):
        if os.environ.get(var):
            return Path(os.environ[var], sub, "openprocyon/assets")
    sys.exit("no XDG_DATA_HOME, HOME or APPDATA to put the assets in: pass --out")


def read_env(path):
    """KEY=value lines of a .env file (optional quotes, # comments)."""
    env = {}
    if path.is_file():
        for line in path.read_text().splitlines():
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
            print(
                f"skip {s}: {err}"
            )  # wrecks/debris are not combat ships; anything else is an exporter bug
            continue
        scene = json.loads((out / f"{s}.json").read_text())["scene"]
        glb = out / "models/ships" / f"{scene}.glb"
        if (
            not glb.is_file()
            and not tool("tools.mdb_to_glb", root, scene, glb, quiet=True)[0]
        ):
            (out / f"{s}.json").unlink(missing_ok=True)
            print(f"skip {s} (no mesh)")
            continue
        ships.append(s)
    (out / "ships.json").write_text(json.dumps(ships) + "\n")


def models(root, out):
    for kind, scenes in (("sky", ["skybox_07"]), ("scenery", SCENERY)):
        for scene in scenes:
            if not (out / "models" / kind / f"{scene}.glb").is_file():
                tool(
                    "tools.mdb_to_glb",
                    root,
                    scene,
                    out / "models" / kind / f"{scene}.glb",
                )
    for part in ("COMPASS", "ARROW"):
        name = f"SELECTION_INDICATOR_{part}_SMALL"
        tool("tools.mdb_to_glb", root, name, out / "models/ui" / f"{name}.glb")


def sounds(root, out):
    """Original Ogg Vorbis, copied with lowercase names (WOT names differ in case from the files)."""
    (out / "sound").mkdir(exist_ok=True)
    files = sorted((root / "Sound/SoundEffects").glob("*.ogg")) + sorted(
        (root / "Sound/GUI").glob("*.ogg")
    )
    for f in files + [root / "Sound/Music" / m for m in MUSIC]:
        if not (out / "sound" / f.name.lower()).is_file():
            shutil.copyfile(f, out / "sound" / f.name.lower())


def flags(root, out):
    """Ship flags, converted to PNG."""
    from PIL import Image

    (out / "flags").mkdir(exist_ok=True)
    for f in (root / "BD_FlagTextures").glob("*.tga"):
        Image.open(f).convert("RGBA").save(out / "flags" / (f.stem.lower() + ".png"))


def steps(root, out, want, part=lambda i, k: None):
    def ship_files():
        return [out / f"{s}.json" for s in json.loads((out / "ships.json").read_text())]

    return [
        ("ships", lambda: export_ships(root, out, want, part)),
        ("models", lambda: models(root, out)),
        ("hud", lambda: tool("tools.export_hud", root, out)),
        ("hud rects", lambda: tool("tools.trim_hud_rects", out)),
        # Waypoint/course art lives in BD_Textures (DDS), not BD_GUITextures.
        ("waypoint art", lambda: tool("tools.export_waypoint_art", root, out)),
        ("alpha bleed", lambda: tool("tools.fix_alpha_bleed", out / "hud/tex")),
        ("slices", lambda: tool("tools.make_slices", out)),
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
    ]


def main(argv):
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
        names = sorted(f.stem for f in (root / "WorldObjects").glob("Ship_*.wot"))
        want = FIRST + [n for n in names if n not in FIRST]
    for kind in ("ships", "sky", "scenery", "ui"):
        (out / "models" / kind).mkdir(parents=True, exist_ok=True)
    progress = Progress()
    todo = steps(root, out, want, progress.part)
    progress.names = [name for name, _ in todo]
    for n, (name, run) in enumerate(todo, 1):
        progress.step(n)
        try:
            run()
        except StepFailed as e:
            sys.exit(f"import failed at step {name!r}: {e}")
    print(f"done: {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
