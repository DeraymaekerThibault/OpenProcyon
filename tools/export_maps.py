"""Export World/*.twt skirmish maps -> assets/maps/<id>.json + index.json, scenery/skybox GLBs, map backdrops, music.

Per map: English name/description (Strings/English_GameStrings.txt), MustAssembleFleet, players (name, team, start point,
forward vector, FormationType), skybox mesh, music track (first 'Play Music Track'), backdrop texture, and the static
objects (islands, asteroids, bases) whose WOT names a Mesh Scene. Positions stay in game axes (x starboard, y fwd, z up).

usage: python -m tools.export_maps <game_root> <assets_dir>
"""

import functools
import json
import re
import shutil
import sys
from pathlib import Path

import numpy as np
from PIL import Image

from tools.gamefs import ci_file, ci_files
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.msb import parse as parse_msb
from tools.wot import blocks, fields, objects, parse_text, properties, records


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


@functools.cache
def mesh_scene(root, typ):
    try:
        nodes, _ = parse_text(
            ci_file(Path(root) / "WorldObjects", f"{typ}.wot").read_text(
                encoding="latin1"
            )
        )
        scene = next(
            (n["value"] for n in properties(nodes) if n["name"] == "Mesh Scene Name"),
            None,
        )
    except FileNotFoundError:
        return None
    return scene or None


def parse_twt(text):
    """Read map fields through the shared WOT tree, tolerating unsupported properties."""
    nodes, _ = parse_text(text)
    # Keep the first occurrence, matching the old scanner for world-level metadata.
    metadata = {}
    for n in properties(nodes):
        metadata.setdefault(n["name"], n["value"])
    actions = []

    def visit(children):
        actions.extend(fields(r) for r in records(children, "Type"))
        for n in children:
            if n["kind"] == "block":
                if n["name"] == "Action List":
                    actions.append(fields(n["children"]))
                else:
                    visit(n["children"])

    visit(nodes)
    m = dict(
        name_id=metadata.get("WorldNameID"),
        desc_id=metadata.get("World Description"),
        assemble=metadata.get("MustAssembleFleet", False),
        sky=metadata.get("Skybox mesh name"),
        music=next(
            (
                a.get("File Name")
                for a in actions
                if a.get("Type") == "Play Music Track"
            ),
            None,
        ),
        backdrop=metadata.get("Backdrop Texture Name"),
        # Lighting the map authors (RGBA colours, roof light direction); the retail lighting model itself is not recovered.
        lighting=dict(
            ambient=metadata.get("Ambient Light"),
            roof_direction=metadata.get("Vector for roof light orientation"),
            floor=metadata.get("Hemispherical floor light color"),
            roof=metadata.get("Hemispherical roof light color"),
        ),
        alliance_change=metadata.get("Is Alliance Change Allowed", False),
        islands_make_sounds=metadata.get("Islands Make Sounds", True),
        players=[],
        objects=list(objects(nodes)),
        teams=[],
        ship_names={},
        primaries=[],  # lists, not sets: export_campaign writes this dict as JSON
    )
    size, buf = metadata.get("World Size - Max"), metadata.get("World Buffer Size")
    m["world_half"] = size[0] - buf if size is not None and buf is not None else None
    for b in list(blocks(nodes, "Team List - Element"))[
        : metadata.get("Team List - Size", 0)
    ]:
        f = fields(b["children"])
        m["teams"].append(
            dict(name_id=f["Team Name ID"], race=f["Race"], lock=f["Race Lock"])
        )
    for b in blocks(nodes, "Player"):
        f = fields(b["children"])
        if "StartPoint" in f and "StartPointForwardVector" in f:
            m["players"].append(
                dict(
                    name=f["Name"],
                    team=f.get("TeamIndex", -1),
                    start=f["StartPoint"],
                    forward=f["StartPointForwardVector"],
                    formation=f.get("FormationType", 0),
                    playable=f.get("IsPlayable", False),
                    race=f.get("Race", 4),
                    color=f.get("Color"),
                )
            )
    for a in actions:
        if a.get("Type") == "*State Init* Setup Ship":
            m["ship_names"][a["World Object ID"]] = (
                a.get("Localized Ship Name", ""),
                a.get("Ship Name", ""),
            )
            if str(a.get("Primary Ship", "")).upper() == "TRUE":
                m["primaries"].append(a["World Object ID"])
    return m


def victory_rules(nodes, unit_names=None):
    """The map's own end-game rules: [{name, if: [condition fields], winner: team name id | None, end: {winner, loser} | None}].
    A rule counts when one of its actions ends the game or names a winning team; its conditions all have to hold. Conditions that
    name a Group/Unit also carry the ids of the world objects in that group (`objects`; "Group,Unit" picks the one unit by its
    ship name, `unit_names`: id -> script name) and, for a named volume, its polygon (`polygon`, game x/y points).
    """
    volumes = {}
    for b in blocks(nodes, "World Polygons Vectors - Element"):
        volumes[fields(b["children"]).get("Name")] = [
            n["value"]
            for n in b["children"]
            if n.get("kind") == "property"
            and n["name"] == "Points"
            and n["type"] == "Coord"
        ]
    groups = {}
    for g in blocks(nodes, "Group"):
        name = fields(g["children"]).get("Name")
        groups[name] = [
            n["value"]
            for n in g["children"]
            if n.get("name") == "World Object IDs - Element"
        ]
    rules = []
    for n in next(blocks(nodes, "World Rules"), {"children": []})["children"]:
        if n["kind"] == "property" and n["name"] == "Rule Name":
            rules.append(dict(name=n["value"], conditions=[], actions=[]))
        elif (
            rules
            and n["kind"] == "block"
            and n["name"] in ("Condition List", "Action List")
        ):
            key = "conditions" if n["name"] == "Condition List" else "actions"
            rules[-1][key].append(fields(n["children"]))
    out = []
    for r in rules:
        wins = [a for a in r["actions"] if a.get("Type") == "Team X Wins"]
        end = [a for a in r["actions"] if a.get("Type") == "End Game"]
        if not (wins or end):
            continue
        conds = []
        for c in r["conditions"]:
            c = dict(c)
            ref = (
                c.get("Group/Unit Name")
                or c.get("Group/Unit")
                or c.get("Group Name")
                or c.get("Unit Name")
            )
            if ref in groups:
                c["objects"] = groups[ref]
            elif isinstance(ref, str) and "," in ref:
                group, unit = ref.split(",", 1)
                c["objects"] = [
                    i
                    for i in groups.get(group, [])
                    if ((unit_names or {}).get(i) or "").lower() == unit.strip().lower()
                ]
            if c.get("Volume Name") in volumes:
                c["polygon"] = volumes[c["Volume Name"]]
            conds.append(c)
        out.append(
            dict(
                name=r["name"],
                **{"if": conds},
                winner=wins[0].get("Team Name") if wins else None,
                end=(
                    dict(
                        winner=end[0].get("Winner - Custom Message String ID"),
                        loser=end[0].get("Loser - Custom Message String ID"),
                    )
                    if end
                    else None
                ),
            )
        )
    return out


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
            raise FileNotFoundError(f"no layer meshes in MSB for {scene}") from None
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
    A transparent seam texel between two visible neighbours takes the colour of the texel before it.
    """
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
    "Crew Select Screen",
    "Arms Selection Screen",
    "Multiplayer Stats Screen",
    "Journal Screen",
]


def main(root, out):
    root, out = Path(root), Path(out)
    (out / "maps").mkdir(parents=True, exist_ok=True)
    (out / "menu").mkdir(parents=True, exist_ok=True)
    text = strings(root)
    index = []
    for f in sorted(ci_files(root / "World", "*.twt")):
        m = parse_twt(f.read_text(encoding="latin1"))
        m["victory"] = victory_rules(
            parse_text(f.read_text(encoding="latin1"))[0],
            {i: ship_name(text, n) for i, n in m["ship_names"].items()},
        )
        mid = re.sub(r"\W+", "_", f.stem).lower()
        m.update(
            id=mid,
            name=text.get(m["name_id"], f.stem),
            description=text.get(m["desc_id"], ""),
        )
        # Historical maps pre-place each player's fleet (PlayerIndex = index in the player list).
        m["ships"] = [
            {
                "id": o["id"],
                "type": o["type"],
                "pos": o["pos"],
                "player": o["player"],
                "name": ship_name(text, m["ship_names"].get(o["id"])),
                # The map's Setup Ship rule picks each player's command ship.
                **({"primary": True} if o["id"] in m["primaries"] else {}),
            }
            for o in m["objects"]
            if o["type"].startswith("Ship_")
            and o["player"] is not None
            and o["player"] >= 0
        ]
        del m["ship_names"], m["primaries"]
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
            (
                "IDGS_TPFRONTENDTEXT",
                "IDGS_TPWORLDNAMES_",
                "IDGS_TPTEAMNAMES_",
                "IDGS_TPINGAMEMESSAGE_",
                "IDGS_TPDISPLAYABLESHIPTYPENAME_",
            )
        )
    }
    (out / "menu" / "strings.json").write_text(json.dumps(keep))


if __name__ == "__main__":
    main(*sys.argv[1:3])
