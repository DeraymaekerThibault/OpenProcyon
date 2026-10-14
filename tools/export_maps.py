"""Export World/*.twt skirmish maps -> assets/maps/<id>.json + index.json, scenery/skybox GLBs, map backdrops, music.

Per map: English name/description (Strings/English_GameStrings.txt), MustAssembleFleet, players (name, team, start point,
forward vector, FormationType), skybox mesh, music track (first 'Play Music Track'), backdrop texture, and the static
objects (islands, asteroids, bases) whose WOT names a Mesh Scene. Positions stay in game axes (x starboard, y fwd, z up).

usage: python -m tools.export_maps <game_root> <assets_dir>
"""

import json
import re
import shutil
import sys
from pathlib import Path
import numpy as np
from PIL import Image
from tools.mdb import ci_file, ci_files
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.msb import parse as parse_msb

VEC = r"\(\s*([-\d.e]+),\s*([-\d.e]+),\s*([-\d.e]+)\s*\)"


def strings(root):
    out, key = {}, None
    for line in (
        (Path(root) / "Strings" / "English_GameStrings.txt")
        .read_text(encoding="latin1")
        .splitlines()
    ):
        line = line.strip()
        if line.startswith("IDGS_"):
            key = line
        elif key and line.startswith('"'):
            out[key] = line.strip('"')
            key = None
    return out


def mesh_scene(root, typ, cache={}):
    if typ not in cache:
        try:
            m = re.search(
                r"Mesh Scene Name String '([^']*)'",
                ci_file(Path(root) / "WorldObjects", f"{typ}.wot").read_text(
                    encoding="latin1"
                ),
            )
            cache[typ] = m.group(1) if m and m.group(1) else None
        except FileNotFoundError:
            cache[typ] = None
    return cache[typ]


def parse_twt(text):
    """Pulls the fields the remake uses out of a .twt; tolerant line scanner (the format is the WOT text format)."""

    def g(pat):
        return (re.search(pat, text) or [None, None])[1]

    m = {
        "name_id": g(r"WorldNameID String '([^']*)'"),
        "desc_id": g(r"World Description String '([^']*)'"),
        "assemble": g(r"MustAssembleFleet Bool (\w+)") == "True",
        "sky": g(r"Skybox mesh name String '([^']*)'"),
        "music": g(r"'Play Music Track'\s*\n\s*File Name String '([^']*)'"),
        "backdrop": g(r"Backdrop Texture Name String '([^']*)'"),
        "players": [],
        "objects": [],
        "teams": [],
    }
    # The star map shows the playable square: world size minus the border buffer.
    size, buf = (
        g(r"World Size - Max Vector3\( ([-\d.]+)"),
        g(r"World Buffer Size Float ([\d.]+)"),
    )
    m["world_half"] = float(size) - float(buf) if size and buf else None
    # WorldInfo team list: the fleet type (Race: 0 Navy, 2 Pirate, 3 Procyon) of each team and whether the lobby may change it.
    n_teams = int(g(r"Team List - Size Int (\d+)") or 0)
    for t in list(
        re.finditer(
            r"Team List - Element\s*\{\s*Team Name ID String '([^']*)'\s*Race Int (-?\d+)\s*Race Lock Bool (\w+)",
            text,
        )
    )[:n_teams]:
        m["teams"].append(
            {
                "name_id": t.group(1),
                "race": int(t.group(2)),
                "lock": t.group(3) == "True",
            }
        )
    for b in re.finditer(
        r"\d+ Player\s*\{\s*Name String '([^']*)'(.*?)FlagIndex", text, re.S
    ):
        body = b.group(2)
        s, f = (
            re.search(r"StartPoint Vector3" + VEC, body),
            re.search(r"StartPointForwardVector Vector3" + VEC, body),
        )
        team, form = (
            re.search(r"TeamIndex Int (-?\d+)", body),
            re.search(r"FormationType Int (-?\d+)", body),
        )
        race = re.search(r"\bRace Int (-?\d+)", body)
        if s and f:
            m["players"].append(
                {
                    "name": b.group(1),
                    "team": int(team.group(1)) if team else -1,
                    "start": [float(x) for x in s.groups()],
                    "forward": [float(x) for x in f.groups()],
                    "formation": int(form.group(1)) if form else 0,
                    "playable": "IsPlayable Bool True" in body,
                    "race": int(race.group(1)) if race else 4,
                }
            )
    # Top-level objects: an optional "ID Int", a tab-indented "Type String" and its State block.
    for o in re.finditer(
        r"(?:\n\tID Int (\d+)\s*)?\n\tType String '([^']+)'\s*\n\t\d+ State\s*\{\s*HasState Bool \w+\s*Position Vector3"
        + VEC
        + r"\s*Orientation Matrix33\(([^)]*)\)(?:\s*PlayerIndex Int (-?\d+))?",
        text,
    ):
        m["objects"].append(
            {
                "type": o.group(2),
                "pos": [float(x) for x in o.groups()[2:5]],
                "rot": [float(x) for x in o.group(6).split(",")],
                "player": int(o.group(7)) if o.group(7) else None,
                "id": int(o.group(1)) if o.group(1) else None,
            }
        )
    # Historical maps name their ships in the world rules: "Setup Ship" actions carry the object id and the localized name id.
    m["ship_names"] = {}
    for a in re.finditer(
        r"Type String '\*State Init\* Setup Ship'\s*World Object ID Int (\d+)\s*Ship Name String '([^']*)'(.*?)\}",
        text,
        re.S,
    ):
        loc = re.search(r"Localized Ship Name String '([^']*)'", a.group(3))
        m["ship_names"][int(a.group(1))] = (loc.group(1) if loc else "", a.group(2))
    return m


def ship_name(text, names):
    """(localized id, script name) -> the English name the game shows (localized when it exists, else the script's)."""
    loc, plain = names or ("", "")
    return text.get(loc) or plain or None


def convert_scene(root, scene):
    """Some scenery uses MSB node mesh names instead of <scene>_Layer*.mdb."""
    try:
        return convert(root, scene)
    except FileNotFoundError:
        nodes = parse_msb(
            ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
        )
        files = [
            f
            for n in nodes
            for f in ci_files(Path(root) / "BD_Meshes", f"{n.name}.mdb")
        ]
        if not files and scene.startswith(
            "Blackhole"
        ):  # empty MSB; the mesh is named Blackhole_<size>_Layer1.mdb
            files = ci_files(
                Path(root) / "BD_Meshes", f"Blackhole_{scene[9:]}_Layer*.mdb"
            )
        if not files:
            raise FileNotFoundError(f"no layer meshes in MSB for {scene}")
        return convert(root, scene, files=files)


def tiles(root, name):
    """Reassembles a GUI texture split into <name>_<col>_<row>.tga tiles."""
    t = {}
    for f in ci_files(Path(root) / "BD_GUITextures", f"{name}_*_*.tga"):
        c, r = map(int, f.stem[len(name) + 1 :].split("_"))
        t[(c, r)] = Image.open(f).convert("RGBA")
    if not t:
        return None
    cols, rows = max(k[0] for k in t) + 1, max(k[1] for k in t) + 1
    w = sum(t[(c, 0)].size[0] for c in range(cols))
    h = sum(t[(0, r)].size[1] for r in range(rows))
    im, x = Image.new("RGBA", (w, h)), 0
    xs, ys = [], []
    for c in range(cols):
        y = 0
        for r in range(rows):
            im.paste(t[(c, r)], (x, y))
            y += t[(c, r)].size[1]
            ys.append(y)
        x += t[(c, 0)].size[0]
        xs.append(x)
    return fill_seams(im, xs[:-1], ys[: rows - 1])


def fill_seams(im, xs, ys):
    """Retail tiles leave their last column/row transparent; pasted edge to edge that shows as a seam line inside the image.
    A transparent seam texel between two visible neighbours takes the colour of the texel before it."""
    a = np.array(im)
    for x in xs:
        gap = (a[:, x - 1, 3] == 0) & (a[:, x - 2, 3] > 0) & (a[:, x, 3] > 0)
        a[gap, x - 1] = a[gap, x - 2]
    for y in ys:
        gap = (a[y - 1, :, 3] == 0) & (a[y - 2, :, 3] > 0) & (a[y, :, 3] > 0)
        a[y - 1, gap] = a[y - 2, gap]
    return Image.fromarray(a)


def parse_tst(text):
    """Screens/*.tst -> [{impl, rect [l,t,r,b] in 800x600, font, text, color, enabled}] in file order."""
    els = []
    for line in text.splitlines():
        line = line.split("#")[0].strip()
        if line.startswith("ScreenElementType"):
            els.append({})
        elif not els:
            continue
        elif m := re.match(r"ImplName String '(.*)'", line):
            els[-1]["impl"] = m.group(1)
        elif m := re.match(r"ReferenceArea Rect\(([^)]*)\)", line):
            els[-1]["rect"] = [int(x) for x in m.group(1).split(",")]
        elif m := re.match(r"Font String '(.*)'", line):
            els[-1]["font"] = m.group(1)
        elif m := re.match(r"Text String '(.*)'", line):
            els[-1]["text"] = m.group(1)
        elif m := re.match(r"Color Colour\(([^)]*)\)", line):
            els[-1]["color"] = [float(x) for x in m.group(1).split(",")]
        elif m := re.match(r"Enabled Bool (\w+)", line):
            els[-1]["enabled"] = m.group(1) == "True"
    return els


SCREENS = [
    "Mainmenu",
    "SinglePlayer Menu",
    "Create Skirmish Game Screen",
    "Skirmish Game Lobby",
    "Ship Selection Screen",
    "Load Campaign Screen",
    "InGameMenu_Default",
    "InGameMenu_Load_Campaign",
    "Tutorial Screen",
]


def main(root, out):
    root, out = Path(root), Path(out)
    (out / "maps").mkdir(parents=True, exist_ok=True)
    (out / "menu").mkdir(parents=True, exist_ok=True)
    text = strings(root)
    index = []
    for f in sorted(ci_files(root / "World", "*.twt")):
        m = parse_twt(f.read_text(encoding="latin1"))
        mid = re.sub(r"\W+", "_", f.stem).lower()
        m.update(
            id=mid,
            name=text.get(m["name_id"], f.stem),
            description=text.get(m["desc_id"], ""),
        )
        # Historical maps pre-place each player's fleet (PlayerIndex = index in the player list).
        m["ships"] = [
            {
                "type": o["type"],
                "pos": o["pos"],
                "player": o["player"],
                "name": ship_name(text, m["ship_names"].get(o["id"])),
            }
            for o in m["objects"]
            if o["type"].startswith("Ship_")
            and o["player"] is not None
            and o["player"] >= 0
        ]
        del m["ship_names"]
        for o in m["objects"]:
            o["scene"] = mesh_scene(root, o["type"])
        # Scenery only (incl. black holes): ships, bullets (mines), animals and nebulae are not static meshes.
        m["objects"] = [
            o
            for o in m["objects"]
            if o["scene"]
            and o["type"].startswith(
                ("Island", "Asteroid", "Base_", "Terrain_BlackHole")
            )
        ]
        for scene in {o["scene"] for o in m["objects"]} | (
            {m["sky"].rsplit("_Layer", 1)[0]} if m["sky"] else set()
        ):
            dst = model_path(out, scene, mkdir=True)
            if not dst.exists():
                try:
                    dst.write_bytes(convert_scene(root, scene))
                except (FileNotFoundError, ValueError) as e:
                    print(f"skip {scene}: {e}")
        m["objects"] = [o for o in m["objects"] if model_path(out, o["scene"]).exists()]
        if m["music"]:
            src = ci_files(root / "Sound" / "Music", m["music"] + ".ogg")
            if src and not (out / "sound" / src[0].name.lower()).exists():
                shutil.copy(src[0], out / "sound" / src[0].name.lower())
        if (
            m["backdrop"]
            and not (out / "menu" / f"{m['backdrop'].lower()}.png").exists()
        ):
            im = tiles(root, m["backdrop"])
            if im:
                im.save(out / "menu" / f"{m['backdrop'].lower()}.png")
        (out / "maps" / f"{mid}.json").write_text(json.dumps(m))
        index.append({"id": mid, "name": m["name"], "assemble": m["assemble"]})
        print(f"{f.stem}: {len(m['players'])} players, {len(m['objects'])} objects")
    (out / "maps" / "index.json").write_text(json.dumps(index))
    for n in [
        "FrontEndScreen0",
        "FrontEndScreen1",
        "FrontEndScreen2",
        "FrontEndScreen3",
        "FrontEndScreen4",
        "FrontEndMap",
        "FrontEndDialogScreen",
        "GameLobby",
        "ShipScreen",
        "FE_ShipScreen_bg",
        "FE_Ingame_Window",
    ]:
        if not (out / "menu" / f"{n.lower()}.png").exists():
            im = tiles(root, n)
            if im:
                im.save(out / "menu" / f"{n.lower()}.png")
    for n in SCREENS:
        els = parse_tst(
            ci_file(root / "Screens", f"{n}.tst").read_text(encoding="latin1")
        )
        (out / "menu" / f"{re.sub(chr(32), '_', n).lower()}.json").write_text(
            json.dumps(els)
        )
    # Menu strings used by the front end (Screens/*.tst text IDs).
    keep = {
        k: v
        for k, v in text.items()
        if k.startswith(
            ("IDGS_TPFRONTENDTEXT", "IDGS_TPWORLDNAMES_", "IDGS_TPTEAMNAMES_")
        )
    }
    (out / "menu" / "strings.json").write_text(json.dumps(keep))


if __name__ == "__main__":
    main(*sys.argv[1:3])
