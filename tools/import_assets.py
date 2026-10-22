"""Import: convert the original install into the game's asset folder. Nothing from the install is modified.
usage: python -m tools.import_assets [--game DIR] [--out DIR] [Ship_Name ...]   (default ships below; first is the default player ship)
  --game  the original install (default: PROCYON_GAME_ROOT from the environment or .env)
  --out   where the converted assets go (default: assets/ in this checkout; the game also finds them in
          ${XDG_DATA_HOME:-~/.local/share}/openprocyon/assets or %APPDATA%\\openprocyon\\assets, see assets_dir() in game/src/lib.rs)
Each step prints `[n/total] name` first, so a front end can show progress. Exits non-zero on the first failing step."""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
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


def tool(*args, quiet=False):
    """Run `python -m tools.<args>` from the checkout; with quiet, return (ok, last stderr line) instead of failing."""
    cmd = [sys.executable, "-m", *map(str, args)]
    if not quiet:
        if subprocess.run(cmd, cwd=REPO).returncode:
            raise StepFailed(" ".join(map(str, args[:2])))
        return True, ""
    r = subprocess.run(
        cmd, cwd=REPO, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True
    )
    return r.returncode == 0, (r.stderr.strip().splitlines() or [""])[-1]


def read_env(path):
    """KEY=value lines of a .env file (optional quotes, # comments)."""
    env = {}
    if path.is_file():
        for line in path.read_text().splitlines():
            key, sep, value = line.strip().partition("=")
            if sep and not key.startswith("#"):
                env[key.strip()] = value.strip().strip("\"'")
    return env


def export_ships(root, out, want):
    ships = []
    for s in want:
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


def steps(root, out, want):
    def ship_files():
        return [out / f"{s}.json" for s in json.loads((out / "ships.json").read_text())]

    return [
        ("ships", lambda: export_ships(root, out, want)),
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
        or read_env(REPO / ".env").get("PROCYON_GAME_ROOT")
    )
    if not game:
        sys.exit("pass --game <install dir> or set PROCYON_GAME_ROOT")
    # Absolute paths: the tools run from the checkout.
    root, out = Path(game).resolve(), Path(out).resolve() if out else REPO / "assets"
    if not (root / "WorldObjects").is_dir():
        sys.exit(f"not a game install (no WorldObjects/): {root}")
    if not want:
        names = sorted(f.stem for f in (root / "WorldObjects").glob("Ship_*.wot"))
        want = FIRST + [n for n in names if n not in FIRST]
    for kind in ("ships", "sky", "scenery", "ui"):
        (out / "models" / kind).mkdir(parents=True, exist_ok=True)
    todo = steps(root, out, want)
    for n, (name, run) in enumerate(todo, 1):
        print(f"[{n}/{len(todo)}] {name}", flush=True)
        try:
            run()
        except StepFailed as e:
            sys.exit(f"import failed at step {name!r}: {e}")
    print(f"done: {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
