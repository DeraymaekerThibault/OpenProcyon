"""Export every single-player mission (World/SinglePlayer/*.twt): placements, paths, volumes, objectives and the rule list as data.
Never reads SavedGames.
usage: python -m tools.export_campaign <game_root> <assets_dir>
"""

import json
import math
import re
import shutil
import sys
from pathlib import Path

from tools import ogg
from tools.export_campaign_media import PLAYABLE
from tools.export_combat import export as export_ship
from tools.export_environment import export_environment
from tools.export_maps import convert_scene, mesh_scene, parse_twt, strings, tiles
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.wot import blocks, fields, objects, parse_text, properties

# Scenes whose BD_Meshes layers carry another name than the scene.
SOURCE = "World/SinglePlayer/Mission01.twt"  # set per mission by main()


def parse_mission(text):
    nodes, diagnostics = parse_text(text, SOURCE)
    # Unsupported property types are tolerated; structural errors are not.
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
    placements = list(objects(nodes))
    ships = []
    for o in placements:
        if not o["type"].startswith("Ship_") or o["id"] is None:
            continue
        setup = setups.get(o["id"], {})
        ships.append(
            dict(
                id=o["id"],
                type=o["type"],
                pos=o["pos"],
                rot=o["rot"],
                name=setup.get("Ship Name", o["type"]),
                group=setup.get("Player/Owner"),
                primary=setup.get("Primary Ship") == "TRUE",
            )
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
    for b in blocks(nodes, "World Polygons Vectors - Element"):
        volumes[fields(b["children"])["Name"]] = [
            n["value"]
            for n in b["children"]
            if n.get("kind") == "property"
            and n["name"] == "Points"
            and n["type"] == "Coord"
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

    def animals_of(prefix, default_group):
        return [
            dict(
                id=o["id"],
                type=o["type"],
                pos=o["pos"],
                rot=o["rot"],
                group=groups.get(o["id"], default_group),
            )
            for o in placements
            if o["type"].startswith(prefix) and o["id"] is not None
        ]

    animals = animals_of("Animal_SpaceWhale", "Whales")
    dragons = animals_of("Animal_SpaceDragon", "Dragons")
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
        [names[f["Player0"]], names[f["Player1"]]]
        for b in blocks(nodes, "PlayerAllianceInfoVector - Element")
        for f in [fields(b["children"])]
    ]
    meta = {}
    for n in properties(nodes):
        meta.setdefault(n["name"], n["value"])

    def pool(name):
        return [n["value"] for n in properties(nodes) if n["name"] == name]

    return dict(
        source=SOURCE,
        # The crew and arms the mission offers (the Crew/Arms Select screens): authored lists, not a save artefact.
        crew_pool=pool("World Crew List - Element"),
        arms_pool=pool("World Arms List - Element"),
        play_end_movie=meta.get("PlayEndMovie", False),
        journal_music=meta.get("Journal Music Name"),
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
    nodes, _ = parse_text(text, SOURCE)
    out = {}
    for b in blocks(nodes, "Speech Event List - Element"):
        f = fields(b["children"])
        out[f["Name"]] = dict(
            file=f.get("Sound FileName"),
            text_id=f.get("TextStringID"),
            face=f.get("FaceTexture"),
            color=f.get("Text Color"),
            head=f.get("TalkingHeadLocation"),
            speaker=f.get("SpeakerID"),
        )
    return out


def main(root, out):
    root, out = Path(root), Path(out)
    for twt in sorted((root / "World/SinglePlayer").glob("*.twt")):
        export_mission(root, out, twt)
    for twt in sorted((root / "World").glob("*.twt")):
        export_skirmish_script(out, twt)


def export_skirmish_script(out, twt):
    """A skirmish map's own rule script (convoys, timers, points, ...) in the mission format -> campaign/<id>_script.json;
    pack_layout moves it to maps/<id>/script.json. The map stays a skirmish map (only mission.json makes a mission)."""
    global SOURCE
    SOURCE = f"World/{twt.name}"
    mid = re.sub(r"\W+", "_", twt.stem).lower()  # the map id export_maps gives it
    script = parse_mission(twt.read_text(encoding="latin1"))
    script.pop("object_groups")
    (out / "campaign").mkdir(exist_ok=True)
    (out / f"campaign/{mid}_script.json").write_text(json.dumps(script, indent=2))


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
        duration = ogg.duration(src)
        if not math.isfinite(duration) or duration <= 0:
            raise ValueError(f"Invalid speech duration: {name}")
        subtitle = names["IDGS_TPSPEECHEVENTS000_" + name]
        if mid in PLAYABLE:
            shutil.copyfile(src, out / "sound" / (name.lower() + ".ogg"))
        mission["opening_speech"].append(
            dict(name=name, text=subtitle, duration=duration)
        )
    m = parse_twt(text)
    del m[
        "primaries"
    ]  # skirmish lobby data; missions pick the command ship by their rules
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
                    # Layer names come from the MSB (mdb_to_glb.scene_layers).
                    model_path(out, scene, mkdir=True).write_bytes(convert(root, scene))
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
