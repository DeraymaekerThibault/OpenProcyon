"""Armed bases and islands: assets/ships/<Base_*|Island_*|Asteroid_*_Armed>/{ship.json,geometry.json} for every Base_*/Island_*/Asteroid_*_Armed world object that places guns
(island gun towers, the volcanoes, the pirate asteroids of mission 11 and Convoy Raid; those are island objects in the original).
Same layout as a ship (pack_data/pack_layout) so the game loads it as a class; no model.glb: the base's visual is its map scenery GLB (models/bases).
New gun/bullet definitions are merged into data/weapons. usage: python3 -m tools.export_bases <game_root> <assets_dir>
"""

import json
from pathlib import Path

from tools import cli, importlog
from tools.export_combat import associations, export, load
from tools.gamefs import ci_files
from tools.jsonio import dump
from tools.pack_data import rnd, sections_by_label


def export_bases(root, assets):
    assets = Path(assets)
    wdir = assets / "data/weapons"
    guns, bullets = (
        json.loads((wdir / "guns.json").read_text(encoding="utf-8")),
        json.loads((wdir / "bullets.json").read_text(encoding="utf-8")),
    )
    wots = [
        w
        for pattern in ("Base_*.wot", "Island_*.wot", "Asteroid_*_Armed.wot")
        for w in ci_files(Path(root) / "WorldObjects", pattern)
    ]
    for wot in wots:
        name = wot.stem
        try:
            nodes = load(root, name)
            armed = any(associations(nodes, "GunPlacement"))
            # A gunless base with a vitalToMission section is still a rules target: mission 6's stockade gate must be shot down.
            # A gunless one without damage sections is drawn scenery only (Base_Cresentia, a render entity): maps draw it from
            # its scene GLB and it needs no class.
            if not armed and not (
                name.startswith("Base_") and any(associations(nodes, "DamageSection"))
            ):
                continue
            j = export(root, name, assets)
        except (KeyError, ValueError, FileNotFoundError, StopIteration) as e:
            importlog.skipped(f"WorldObjects/{name}.wot", f"{type(e).__name__}: {e}")
            continue
        if not j["guns"] and not any(s["vital_mission"] for s in j["sections"]):
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
        ship["sections"] = sections_by_label(j)
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
        (d / "ship.json").write_text(dump(rnd(ship)), encoding="utf-8")
        (d / "geometry.json").write_text(
            dump(
                {
                    s["layer"]: {"aabb": rnd(s["aabb"], 4), "tris": rnd(s["tris"], 4)}
                    for s in j["sections"]
                }
            ),
            encoding="utf-8",
        )
        print("base:", name, len(j["guns"]), "guns")
    (wdir / "guns.json").write_text(dump(rnd(guns)), encoding="utf-8")
    (wdir / "bullets.json").write_text(dump(rnd(bullets)), encoding="utf-8")


if __name__ == "__main__":
    export_bases(*cli.args(__doc__, 2))
