"""Export the nebula / solar storm / meteor storm setups of every world (World/*.twt, World/SinglePlayer/*.twt) into the
matching assets/maps/<id>.json as "nebulae", plus the GeoSolar mesh and the weather sounds.
Each entry: the rule's 'Setup Nebula' fields, its polygon (game x, y) and the cloud/storm/meteor point sets (pos, magnitude).
usage: python -m tools.export_nebulae <game_root> <assets_dir>   (patches existing map files; run after export_maps/export_campaign)
"""

import json
import re
import shutil
import sys
from pathlib import Path

from tools.export_campaign import parse_mission
import tools.export_campaign as campaign
from tools.mdb import ci_files
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.export_combat import load, flat

SOUNDS = [
    "Ambience_SolarStorm_Loop",
    "MeteorStorm_Inside",
    "MeteorStorm_Outside",
    "Ambience_Black_Hole",
]
NONE = ("POINT SET", "EFFECT", "")


def nebulae(text, source):
    campaign.SOURCE = source
    m = parse_mission(text)
    env = m["environment"]
    out = []
    for n in env["nebulae"]:

        def points(key):
            name = n.get(key)
            return [] if name in NONE else env["point_sets"].get(name, [])

        out.append(
            {
                "name": n["New Nebula Name"],
                "polygon": m["volumes"].get(n["Polygon Name"], []),
                "cloud": {
                    "effect": n["Nebula Cloud Effect Name"],
                    "points": points("Nebula Cloud Point Set Name"),
                    "drain": n["Nebula Cloud Energy Drain On/Off"] == "TRUE",
                    "occlusion": n["Nebula Occlusion On/Off"] == "TRUE",
                },
                "storm": {
                    "effect": n["Solar Storm Effect Name"],
                    "points": points("Solar Storm Point Set Name"),
                    "lightning": n["Lightning On/Off"] == "TRUE",
                    "lightning_recharge": n["Lightning Blast Recharge Time"],
                    "winds": n["Rotational Winds On/Off"] == "TRUE",
                    "wind_magnitude": n["Wind Magnitude"],
                    "wind_damage_frequency": n["Solar Storm Wind Damage Frequency"],
                },
                "meteors": {
                    "effect": n["Meteor Shower Effect Name"],
                    "points": points("Meteor Shower Point Set Name"),
                    "on": n["Meteors On/Off"] == "TRUE",
                    "recharge": n["Meteor Strike Recharge Time"],
                },
                "sound_distance": n.get("Ambient sound max distance", 300.0),
            }
        )
    return out


def weather_damage(nodes):
    """Weather uses its own per-material potentials, not gun bullet damage."""
    out = {}
    for n in nodes:
        if n["kind"] != "block":
            continue
        if n["name"] in ("Lightning Damage Potential", "Wind Damage Potential", "Meteor Damage Potential"):
            p = flat(n["children"])
            out[n["name"].split()[0].lower()] = {
                material: [p[f"{key}: {material}"] for key in (
                    "Damage Lower Bound", "Damage Upper Bound", "Chance of Critical Damage", "Chance of Fire", "Initial Fire Strength"
                )]
                for material in ("Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon")
            }
        else:
            out.update(weather_damage(n["children"]))
    return out


def main(root, out):
    root, out = Path(root), Path(out)
    damage = weather_damage(load(root, "Terrain_Nebula"))
    if set(damage) != {"lightning", "wind", "meteor"}:
        raise ValueError("Missing nebula weather damage tables")
    (out / "data").mkdir(parents=True, exist_ok=True)
    (out / "data/nebula.json").write_text(json.dumps(damage, indent=2) + "\n")
    for twt in sorted(
        [*(root / "World").glob("*.twt"), *(root / "World/SinglePlayer").glob("*.twt")]
    ):
        mid = re.sub(r"\W+", "_", twt.stem).lower()
        f = out / "maps" / f"{mid}.json"
        if not f.exists():
            continue
        m = json.loads(f.read_text())
        try:
            m["nebulae"] = nebulae(
                twt.read_text(encoding="latin1"), str(twt.relative_to(root))
            )
        except (ValueError, KeyError, IndexError) as e:  # installs vary (mods); one broken map must not stop the import
            print(f"skip {mid} nebulae: {e}")
            continue
        f.write_text(json.dumps(m))
        print(f"{mid}: {len(m['nebulae'])} nebula setups")
    dst = model_path(out, "GeoSolar", mkdir=True)
    if not dst.exists():
        dst.write_bytes(
            convert(root, "GeoSolar", files=[root / "BD_Meshes/GeoSolar_Layer1.mdb"])
        )
    for s in SOUNDS:
        src = ci_files(root / "Sound/Dialogue", s + ".ogg")
        if src and not (out / "sound" / (s.lower() + ".ogg")).exists():
            shutil.copyfile(src[0], out / "sound" / (s.lower() + ".ogg"))


if __name__ == "__main__":
    main(*sys.argv[1:3])
