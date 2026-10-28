"""Armed bases and islands: assets/ships/<Base_*|Island_*>/{ship.json,geometry.json} for every Base_*/Island_* world object that places guns
(island gun towers, the volcanoes).
Same layout as a ship (pack_data/pack_layout) so the game loads it as a class; no model.glb: the base's visual is its map scenery GLB (models/bases).
New gun/bullet definitions are merged into data/weapons. usage: python3 -m tools.export_bases <game_root> <assets_dir>
"""

import json
import sys
from pathlib import Path

from tools.export_combat import associations, export, load
from tools.gamefs import ci_files
from tools.pack_data import dump, rnd


def export_bases(root, assets):
    assets = Path(assets)
    wdir = assets / "data/weapons"
    guns, bullets = (
        json.loads((wdir / "guns.json").read_text()),
        json.loads((wdir / "bullets.json").read_text()),
    )
    wots = [
        w
        for pattern in ("Base_*.wot", "Island_*.wot")
        for w in ci_files(Path(root) / "WorldObjects", pattern)
    ]
    for wot in wots:
        name = wot.stem
        try:
            if not any(associations(load(root, name), "GunPlacement")):
                continue
            j = export(root, name, assets)
        except (KeyError, ValueError, FileNotFoundError, StopIteration):
            continue
        if not j["guns"]:
            continue
        for k, g in j["gun_defs"].items():
            guns.setdefault(k, g)
        for k, b in j["bullets"].items():
            bullets.setdefault(k, b)
        ship = {
            k: j[k]
            for k in (
                "entity",
                "scene",
                "race",
                "lifeboats",
                "is_lifeboat",
                "response",
                "sphere",
            )
            if k in j
        }
        ship |= dict(
            skirmish=False,
            geometry=name,
            core_hp=j["core_hp"],
            physics={k: v["value"] for k, v in j["physics"].items()},
        )
        ship["sections"] = {
            s["label"]: {
                k: v for k, v in s.items() if k not in ("label", "aabb", "tris")
            }
            for s in j["sections"]
        }
        # The volcanoes name every mount 'default'; the bank (unique per mount) then tells them apart.
        labels = [g["label"] for g in j["guns"]]
        ship["guns"] = {
            (
                g["label"]
                if labels.count(g["label"]) == 1
                else f"{g['label']}_{g['bank']}"
            ): {k: v for k, v in g.items() if k != "label"}
            for g in j["guns"]
        }
        for k in (
            "engines",
            "wakes",
            "flags",
            "explosion_fx",
            "engine_snd",
            "skipped_guns",
            "sight",
            "repair_fx",
        ):
            if k in j:
                ship[k] = j[k]
        d = assets / "ships" / name
        d.mkdir(parents=True, exist_ok=True)
        (d / "ship.json").write_text(dump(rnd(ship)))
        (d / "geometry.json").write_text(
            json.dumps(
                {
                    s["layer"]: {"aabb": rnd(s["aabb"], 4), "tris": rnd(s["tris"], 4)}
                    for s in j["sections"]
                },
                separators=(",", ":"),
            )
        )
        print("base:", name, len(j["guns"]), "guns")
    (wdir / "guns.json").write_text(dump(rnd(guns)))
    (wdir / "bullets.json").write_text(dump(rnd(bullets)))


if __name__ == "__main__":
    export_bases(*sys.argv[1:3])
