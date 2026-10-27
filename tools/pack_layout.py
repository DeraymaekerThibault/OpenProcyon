"""Regroups the exporters' per-type output into one folder per ship and per map (the final build step; idempotent).

  ships/<Id>/ship.json      stats, mounts ("geometry"/"model" name the ship whose geometry.json / model.glb to use; variants share a hull)
  ships/<Id>/geometry.json  per-section hit meshes
  ships/<Id>/model.glb      hull mesh (only in the ship that owns it)
  ships/<Id>/names.json     pool of ship names for open maps (optional)
  ships/<Id>/docks.json     dock points (optional)
  maps/<id>/map.json        scenery, start points, nebulae; "skirmish": true when the skirmish menu lists it
  maps/<id>/mission.json    campaign rules, ships, paths (campaign maps only)
  maps/<id>/media.json      mission speech/messages (campaign maps only)
  maps/<id>/script.json     the map's own rule script, mission format (skirmish maps only)
  maps/<id>/backdrop.png    star-map / lobby picture (copy of menu/<backdrop>.png)
  maps/<id>/speech/*.ogg    dialogue only that mission uses (dialogue shared by several maps: sound/speech/)
  sound/<kind>/*.ogg        weapons, impacts, ships (engines...), ambience, music, ui, speech; the game finds files by name

usage: python3 -m tools.pack_layout <assets dir>"""

import json
import re
import shutil
import sys
from pathlib import Path

from tools.pack_data import dump


def _read(p):
    return json.loads(p.read_text()) if p.exists() else None


def pack_ships(assets):
    data = assets / "data"
    names, docks = (
        _read(data / "ship_names.json") or {},
        _read(data / "docks.json") or {},
    )
    ships = (
        sorted(f.stem for f in (data / "ships").glob("*.json"))
        if (data / "ships").is_dir()
        else []
    )
    owner = {}
    for s in ships:
        scene = json.loads((data / "ships" / f"{s}.json").read_text())["scene"]
        owner.setdefault(
            scene, s
        )  # sorted: the plain hull comes before its _Feather/_NIS/... variants
    for s in ships:
        d = assets / "ships" / s
        d.mkdir(parents=True, exist_ok=True)
        j = json.loads((data / "ships" / f"{s}.json").read_text())
        j["geometry"], j["model"] = s, owner[j["scene"]]
        (d / "ship.json").write_text(dump(j))
        (data / "ships" / f"{s}.json").unlink()
        shutil.move(data / "geometry" / f"{s}.json", d / "geometry.json")
        glb = assets / "models/ships" / f"{j['scene']}.glb"
        if owner[j["scene"]] == s and glb.exists():
            shutil.move(glb, d / "model.glb")
        if s in names:
            (d / "names.json").write_text(dump(names.pop(s)))
        if s in docks:
            (d / "docks.json").write_text(dump(docks.pop(s)))
    for sub in ("ships", "geometry"):
        if (data / sub).is_dir() and not any((data / sub).iterdir()):
            (data / sub).rmdir()
    (data / "ship_names.json").unlink(missing_ok=True)
    (assets / "ships.json").unlink(missing_ok=True)
    if docks:
        (data / "docks.json").write_text(dump(docks))


def pack_maps(assets):
    maps = assets / "maps"
    used = set()
    listed = {m["id"] for m in _read(maps / "index.json") or []}
    for f in sorted(maps.glob("*.json")):
        if f.stem == "index":
            continue
        m = json.loads(f.read_text())
        d = maps / f.stem
        d.mkdir(exist_ok=True)
        if f.stem in listed:
            m["skirmish"] = True
        (d / "map.json").write_text(dump(m))
        f.unlink()
        for src, dst in (
            (assets / "campaign" / f"{f.stem}.json", "mission.json"),
            (assets / "campaign" / f"{f.stem}_media.json", "media.json"),
            (assets / "campaign" / f"{f.stem}_script.json", "script.json"),
        ):
            if src.exists():
                shutil.move(src, d / dst)
        bd = assets / "menu" / f"{(m.get('backdrop') or '').lower()}.png"
        if m.get("backdrop") and bd.exists():
            shutil.copy(bd, d / "backdrop.png")  # several missions share one picture
            used.add(bd)
    (maps / "index.json").unlink(missing_ok=True)
    for bd in used:
        bd.unlink()  # the picture now lives in its map folder
    if (assets / "campaign").is_dir() and not any((assets / "campaign").iterdir()):
        (assets / "campaign").rmdir()


# First matching prefix wins; a name that matches none stays where it is.
SOUND_KINDS = [
    ("speech", ("m0", "m1", "mtut")),
    ("weapons", ("weapon_",)),
    (
        "impacts",
        (
            "damage_",
            "explosion_",
            "ricochet",
            "richocet",
            "ship_collision",
            "ship_destroyed",
        ),
    ),
    ("ships", ("engine_", "ic_", "ship_", "rope_")),
    ("ambience", ("ambience_", "animal_", "solar_", "volcano_", "meteorstorm")),
    (
        "music",
        (
            "btl_",
            "neutrl_",
            "trans_",
            "undrscore_",
            "journal_",
            "frontendmusic",
            "temp_yoho",
        ),
    ),
    ("ui", ("hud_", "fe_", "genericclick", "frontend_", "spinbutton", "startbutton")),
]


def pack_sound(assets):
    sound = assets / "sound"
    flat = sorted(sound.glob("*.ogg")) if sound.is_dir() else []
    users = {}  # file stem -> maps whose JSON names it
    for f in assets.glob("maps/*/*.json"):
        for tok in set(re.findall(r"[A-Za-z0-9_]+", f.read_text())):
            users.setdefault(tok.lower(), set()).add(f.parent.name)
    for f in flat:
        kind = next((k for k, pre in SOUND_KINDS if f.name.startswith(pre)), None)
        if kind is None and f.stem in users:
            kind = "speech"  # NIS/pickup dialogue a mission names (nis_solarstorm, bottlerocket_pickup, hmrigger03)
        if kind is None:
            continue
        dst = sound / kind
        if kind == "speech":
            maps = users.get(f.stem, set())
            if (
                not maps
                and re.match(r"m\d\d", f.stem)
                and (assets / "maps" / f"mission{f.stem[1:3]}").is_dir()
            ):
                maps = {f"mission{f.stem[1:3]}"}
            if len(maps) == 1:
                dst = assets / "maps" / next(iter(maps)) / "speech"
        dst.mkdir(parents=True, exist_ok=True)
        shutil.move(f, dst / f.name)


def pack(assets):
    assets = Path(assets)
    pack_ships(assets)
    pack_maps(assets)
    pack_sound(assets)


if __name__ == "__main__":
    pack(sys.argv[1])
