"""Turns the raw per-ship exports (assets/Ship_*.json, written by export_combat/export_effects) into the editable data layout:

  data/ships/<Name>.json       stats and mounts, hand-editable: flat physics, sections and guns keyed by label, no triangles
  data/geometry/<Name>.json    per-section hit meshes (generated; not meant to be edited)
  data/weapons/guns.json       every gun definition, shared by all ships
  data/weapons/bullets.json    every bullet definition, shared by all ships

The raw Ship_*.json files are deleted afterwards. usage: python3 tools/pack_data.py <assets dir>"""

import json
from pathlib import Path

from tools import cli
from tools.jsonio import dump
from tools.skirmish_ships import SKIRMISH_SHIPS

# Ships offered in the skirmish pickers: tools/skirmish_ships.py (also the importer's first ships); mods opt in with "skirmish": true.
SKIRMISH = set(SKIRMISH_SHIPS)


def rnd(v, nd=6):
    if isinstance(v, float):
        return round(v, nd)
    if isinstance(v, list):
        return [rnd(x, nd) for x in v]
    if isinstance(v, dict):
        return {k: rnd(x, nd) for k, x in v.items()}
    return v


def sections_by_label(j):
    """A ship's sections by label, without the geometry the game reads from the model."""
    return {
        s["label"]: {k: v for k, v in s.items() if k not in ("label", "aabb", "tris")}
        for s in j["sections"]
    }


def pack(assets):
    assets = Path(assets)
    for sub in ("ships", "geometry", "weapons"):
        (assets / "data" / sub).mkdir(parents=True, exist_ok=True)
    guns, bullets = {}, {}
    for f in sorted(assets.glob("Ship_*.json")):
        name, j = f.stem, json.loads(f.read_text(encoding="utf-8"))
        for k, g in j["gun_defs"].items():
            assert guns.setdefault(k, g) == g, f"gun {k} differs between ships"
        for k, b in j["bullets"].items():
            assert bullets.setdefault(k, b) == b, f"bullet {k} differs between ships"
        geometry = {
            s["layer"]: {"aabb": rnd(s["aabb"], 4), "tris": rnd(s["tris"], 4)}
            for s in j["sections"]
        }
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
        ship["skirmish"] = j.get("skirmish", name in SKIRMISH)
        ship["geometry"] = name
        ship["core_hp"] = j["core_hp"]
        ship["physics"] = {k: v["value"] for k, v in j["physics"].items()}
        ship["sections"] = sections_by_label(j)
        ship["guns"] = {
            g["label"]: {k: v for k, v in g.items() if k != "label"} for g in j["guns"]
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
            "cloakable",
            "points",
            "crew_posts",
            "display_name",
            "icon",
        ):
            if k in j:
                ship[k] = j[k]
        (assets / "data/ships" / f"{name}.json").write_text(
            dump(rnd(ship)), encoding="utf-8"
        )
        (assets / "data/geometry" / f"{name}.json").write_text(
            dump(geometry), encoding="utf-8"
        )
        f.unlink()
    (assets / "data/weapons/guns.json").write_text(dump(rnd(guns)), encoding="utf-8")
    (assets / "data/weapons/bullets.json").write_text(
        dump(rnd(bullets)), encoding="utf-8"
    )


if __name__ == "__main__":
    pack(*cli.args(__doc__, 1))
