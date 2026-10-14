"""Export every single-player mission (World/SinglePlayer/*.twt): placements, paths, volumes, objectives and the rule list as data.
Never reads SavedGames.
usage: python -m tools.export_campaign <game_root> <assets_dir>
"""

import json
import re
import shutil
import sys
import subprocess
import math
from pathlib import Path

from tools.export_campaign_media import PLAYABLE
from tools.export_maps import VEC, convert_scene, mesh_scene, parse_twt, strings, tiles
from tools.export_combat import export as export_ship
from tools.export_environment import export_environment
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.source_text import SourceLine
from tools.wot import parse_lines

# Scenes whose BD_Meshes layers carry another name than the scene.
SOURCE = "World/SinglePlayer/Mission01.twt"  # set per mission by main()


def fields(nodes):
    return {n["name"]: n["value"] for n in nodes if n["kind"] == "property"}


def blocks(nodes, name):
    for n in nodes:
        if n["kind"] == "block":
            if n["name"] == name:
                yield n
            yield from blocks(n["children"], name)


def parse_mission(text):
    nodes, diagnostics = parse_lines(
        [SourceLine(SOURCE, i, line) for i, line in enumerate(text.splitlines(), 1)]
    )
    # Matrix33/Colour/Coord are outside the WOT parser's subset; structural errors are not.
    errors = [
        d
        for d in diagnostics
        if d["code"] not in ("unsupported_line", "invalid_property")
    ]
    if errors:
        raise ValueError(errors[:3])
    rules = []
    children = next(blocks(nodes, "World Rules"))["children"]
    for n in children:
        if n["kind"] == "property" and n["name"] == "Rule Name":
            rules.append(
                {
                    "name": n["value"],
                    "line": n["source"]["line"],
                    "conditions": [],
                    "actions": [],
                }
            )
        elif (
            rules
            and n["kind"] == "block"
            and n["name"] in ("Condition List", "Action List")
        ):
            rules[-1][
                "conditions" if n["name"] == "Condition List" else "actions"
            ].append(fields(n["children"]))
        elif rules and n["kind"] == "property":
            rules[-1][n["name"]] = n["value"]
    setups = {
        a["World Object ID"]: a
        for r in rules
        for a in r["actions"]
        if a.get("Type") == "*State Init* Setup Ship"
    }
    ships = []
    for m in re.finditer(
        r"\n\tID Int (\d+)\s*\n\tType String '(Ship_[^']+)'\s*\n\t\d+ State\s*\{(.*?)\n\t\}",
        text,
        re.S,
    ):
        ident, typ, body = m.groups()
        pos = re.search(r"Position Vector3" + VEC, body)
        rot = re.search(r"Orientation Matrix33\(([^)]*)\)", body)
        setup = setups.get(
            int(ident), {}
        )  # ships no rule sets up keep their placement only
        ships.append(
            {
                "id": int(ident),
                "type": typ,
                "pos": list(map(float, pos.groups())),
                "rot": list(map(float, rot[1].split(","))),
                "name": setup.get("Ship Name", typ),
                "group": setup.get("Player/Owner"),
                "primary": setup.get("Primary Ship") == "TRUE",
            }
        )
    paths = {}
    for b in blocks(nodes, "Waypoint Path Info Vector - Element"):
        paths[fields(b["children"])["Waypoint Path Name"]] = [
            n["value"]
            for n in b["children"]
            if n.get("name") == "Waypoint Path Points - Element"
        ]
    points = [
        fields(b["children"]) for b in blocks(nodes, "Objective Point Info - Element")
    ]
    landmarks = {
        fields(b["children"])["Name"]: fields(b["children"])["Position"]
        for b in blocks(nodes, "MapText Point Info - Element")
    }
    map_text = [
        fields(b["children"]) for b in blocks(nodes, "MapText Point Info - Element")
    ]
    tasks = [
        fields(b["children"]) for b in blocks(nodes, "Objective Task Array - Element")
    ]
    volumes = {}
    for b in re.finditer(
        r"\d+ World Polygons Vectors - Element\s*\{(.*?)\}", text, re.S
    ):
        name = re.search(r"Name String '([^']+)'", b[1])[1]
        volumes[name] = [
            list(map(float, p))
            for p in re.findall(r"Points Coord\(\s*([-\d.]+),\s*([-\d.]+)\s*\)", b[1])
        ]
    groups, named_groups = {}, {}
    for group in blocks(nodes, "Group"):
        name = fields(group["children"]).get("Name")
        named_groups[name] = [
            n["value"]
            for n in group["children"]
            if n.get("name") == "World Object IDs - Element"
        ]
        for i in named_groups[name]:
            groups[i] = name
    animals = []
    for m in re.finditer(
        r"\n\tID Int (\d+)\s*\n\tType String '(Animal_SpaceWhale)'\s*\n\t\d+ State\s*\{(.*?)\n\t\}",
        text,
        re.S,
    ):
        ident, typ, body = m.groups()
        pos = re.search(r"Position Vector3" + VEC, body)
        rot = re.search(r"Orientation Matrix33\(([^)]*)\)", body)
        animals.append(
            dict(
                id=int(ident),
                type=typ,
                pos=list(map(float, pos.groups())),
                rot=list(map(float, rot[1].split(","))),
                group=groups.get(int(ident), "Whales"),
            )
        )
    dragons = []
    for m in re.finditer(
        r"\n\tID Int (\d+)\s*\n\tType String '(Animal_SpaceDragon\w*)'\s*\n\t\d+ State\s*\{(.*?)\n\t\}",
        text,
        re.S,
    ):
        ident, typ, body = m.groups()
        pos = re.search(r"Position Vector3" + VEC, body)
        rot = re.search(r"Orientation Matrix33\(([^)]*)\)", body)
        dragons.append(
            dict(
                id=int(ident),
                type=typ,
                pos=list(map(float, pos.groups())),
                rot=list(map(float, rot[1].split(","))),
                group=groups.get(int(ident), "Dragons"),
            )
        )
    point_sets = {}
    for point_set in blocks(nodes, "World Point Sets Vector - Element"):
        points_in_set = []
        for point in blocks(point_set["children"], "World Points - Element"):
            basis = next(blocks(point["children"], "World Point Basis"))
            points_in_set.append(
                dict(
                    pos=fields(basis["children"])["Position"],
                    magnitude=fields(point["children"])["World Point Magnitude"],
                )
            )
        point_sets[fields(point_set["children"])["Name"]] = points_in_set
    nebulae = [
        a
        for r in rules
        for a in r["actions"]
        if a.get("Type") == "*State Init* Setup Nebula"
    ]
    players = parse_twt(text)["players"]
    names = [p["name"] for p in players]
    # PlayerAllianceInfoVector: allied player-index pairs; every other pair of players starts hostile.
    alliances = [
        [names[int(a)], names[int(b)]]
        for a, b in re.findall(
            r"PlayerAllianceInfoVector - Element\s*\{\s*Player0 Int (\d+)\s*Player1 Int (\d+)",
            text,
        )
    ]
    return dict(
        source=SOURCE,
        ships=ships,
        paths=paths,
        points=points,
        tasks=tasks,
        volumes=volumes,
        rules=rules,
        landmarks=landmarks,
        map_text=map_text,
        environment=dict(
            animals=animals, dragons=dragons, nebulae=nebulae, point_sets=point_sets
        ),
        object_groups=groups,
        player_start=players[0] if players else None,
        players=names,
        alliances=alliances,
        groups=named_groups,
    )


def speech_events(text):
    """The TWT 'Speech Event List': rule-facing event name -> sound file, subtitle string id and talking-head sprite."""
    nodes, _ = parse_lines(
        [SourceLine(SOURCE, i, line) for i, line in enumerate(text.splitlines(), 1)]
    )
    out = {}
    for b in blocks(nodes, "Speech Event List - Element"):
        f = fields(b["children"])
        out[f["Name"]] = dict(
            file=f.get("Sound FileName"),
            text_id=f.get("TextStringID"),
            face=f.get("FaceTexture"),
        )
    return out


def main(root, out):
    root, out = Path(root), Path(out)
    for twt in sorted((root / "World/SinglePlayer").glob("*.twt")):
        export_mission(root, out, twt)


def export_mission(root, out, twt):
    global SOURCE
    SOURCE = f"World/SinglePlayer/{twt.name}"
    mid = twt.stem.lower()
    text = twt.read_text(encoding="latin1")
    mission = parse_mission(text)
    object_groups = mission.pop("object_groups")
    export_environment(root, out, mission)
    names = strings(root)
    for task in mission["tasks"]:
        task["text"] = names.get(task.get("TextStringID"), task.get("Name"))
    opening = next((r for r in mission["rules"] if r["name"] == "Opening NIS 2"), None)
    mission["map_text"] = [
        dict(
            name=f["Name"],
            text=names.get(f["DisplayedText"], f["Name"]),
            pos=f["Position"],
            visible=f["Visible"],
        )
        for f in mission["map_text"]
    ]
    mission["opening_speech"] = []
    for action in (opening or {"actions": []})["actions"]:
        if action["Type"] != "Play speech event":
            continue
        name = action["Speech Event Name"]
        src = root / "Sound/Dialogue" / (name + ".ogg")
        duration = float(
            subprocess.check_output(
                [
                    "ffprobe",
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "csv=p=0",
                    str(src),
                ],
                text=True,
            )
        )
        if not math.isfinite(duration) or duration <= 0:
            raise ValueError(f"Invalid speech duration: {name}")
        subtitle = names["IDGS_TPSPEECHEVENTS000_" + name]
        if mid in PLAYABLE:
            shutil.copyfile(src, out / "sound" / (name.lower() + ".ogg"))
        mission["opening_speech"].append(
            dict(name=name, text=subtitle, duration=duration)
        )
    m = parse_twt(text)
    m.update(
        id=mid,
        name=names.get(m["name_id"], twt.stem),
        description=names.get(m["desc_id"], ""),
    )
    # Wrecked hulls (no ship class: no engines or core) are static scenery the rules can still address (teleport, collidable).
    # (A wreck with a ship class, the StarHammer of mission 9, is spawned as a ship instead.)
    classes = (
        set(json.loads((out / "ships.json").read_text()))
        if (out / "ships.json").exists()
        else set()
    )

    def scenery(t):
        return t.startswith(("Island", "Asteroid", "Base_", "Terrain_BlackHole")) or (
            t.startswith("Ship_") and t.endswith("_Wrecked") and t not in classes
        )

    m["objects"] = [
        dict(o, scene=mesh_scene(root, o["type"]))
        for o in m["objects"]
        if scenery(o["type"])
    ]
    for obj in m["objects"]:
        if obj["type"].startswith("Asteroid") or obj["type"].endswith("_Wrecked"):
            obj["group"] = object_groups.get(obj["id"])
    scenes = {o["scene"] for o in m["objects"] if o["scene"]} | (
        {m["sky"].rsplit("_Layer", 1)[0]} if m["sky"] else set()
    )
    for scene in scenes:
        if not model_path(out, scene).exists():
            try:
                model_path(out, scene, mkdir=True).write_bytes(
                    convert_scene(root, scene)
                )
            except FileNotFoundError:
                print(f"{mid}: unresolved scenery {scene}")
    mission["unresolved_scenery"] = [
        o
        for o in m["objects"]
        if not o["scene"] or not model_path(out, o["scene"]).exists()
    ]
    m["objects"] = [
        o for o in m["objects"] if o["scene"] and model_path(out, o["scene"]).exists()
    ]
    registry = json.loads((out / "ships.json").read_text())
    mission["unresolved_ships"] = []
    for typ in sorted({s["type"] for s in mission["ships"]}):
        if typ not in registry:
            try:
                (out / f"{typ}.json").write_text(
                    json.dumps(export_ship(root, typ, out))
                )
                scene = mesh_scene(root, typ)
                if not model_path(out, scene).exists():
                    model_path(out, scene, mkdir=True).write_bytes(
                        convert(root, scene)  # layer names come from the MSB (mdb_to_glb.scene_layers)
                    )
                registry.append(typ)
            except (KeyError, ValueError, FileNotFoundError):
                mission["unresolved_ships"].append(
                    typ
                )  # wrecks and other non-combat hulls
    (out / "ships.json").write_text(json.dumps(registry))
    handoff = next(
        (r for r in mission["rules"] if r["name"].startswith("END Opening NIS")), None
    )
    if handoff:
        mission["handoff_music"] = next(
            (
                a["File Name"]
                for a in handoff["actions"]
                if a["Type"] == "Play Music Track"
            ),
            None,
        )
    for track in [m["music"], mission.get("handoff_music")]:
        src = next((root / "Sound/Music").glob(track + ".ogg"), None) if track else None
        if src and mid in PLAYABLE:
            shutil.copyfile(src, out / "sound" / (track.lower() + ".ogg"))
    (out / f"maps/{mid}.json").write_text(json.dumps(m))
    bd = out / "menu" / f"{(m.get('backdrop') or '').lower()}.png"
    if m.get("backdrop") and not bd.exists() and (im := tiles(root, m["backdrop"])):
        bd.parent.mkdir(parents=True, exist_ok=True)
        im.save(bd)  # star-map picture; pack_layout moves it to maps/<id>/backdrop.png
    (out / "campaign").mkdir(exist_ok=True)
    (out / f"campaign/{mid}.json").write_text(json.dumps(mission, indent=2))
    print(
        f"{mid}: {len(mission['ships'])} ships, {len(mission['rules'])} rules, {len(mission['volumes'])} volumes"
    )


if __name__ == "__main__":
    main(*sys.argv[1:3])
