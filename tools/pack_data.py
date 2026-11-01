"""Turns the raw per-ship exports (assets/Ship_*.json, written by export_combat/export_effects) into the editable data layout:

  data/ships/<Name>.json       stats and mounts, hand-editable: flat physics, sections and guns keyed by label, no triangles
  data/geometry/<Name>.json    per-section hit meshes (generated; not meant to be edited)
  data/weapons/guns.json       every gun definition, shared by all ships
  data/weapons/bullets.json    every bullet definition, shared by all ships

The raw Ship_*.json files are deleted afterwards. usage: python3 tools/pack_data.py <assets dir>"""

import json
import re
import sys
from pathlib import Path

# Ships offered in the skirmish pickers (retail: the single-player fleet screens); mods opt in with "skirmish": true.
SKIRMISH = {
    "Ship_Navy_Cutter",
    "Ship_Navy_Frigate",
    "Ship_Navy_FastFrigate",
    "Ship_Pirate_Schooner",
    "Ship_Pirate_Sloop",
    "Ship_Pirate_Gunboat",
    "Ship_Pirate_Barque",
}

NUM = re.compile(r"\[\s*(-?[\d.eE+-]+(?:,\s*-?[\d.eE+-]+)*)\s*\]")


def rnd(v, nd=6):
    if isinstance(v, float):
        return round(v, nd)
    if isinstance(v, list):
        return [rnd(x, nd) for x in v]
    if isinstance(v, dict):
        return {k: rnd(x, nd) for k, x in v.items()}
    return v


def dump(v):
    """indent=2, but number lists stay on one line."""
    return (
        NUM.sub(
            lambda m: "[" + re.sub(r"\s+", " ", m.group(1)) + "]",
            json.dumps(v, indent=2),
        )
        + "\n"
    )


def pack(assets):
    assets = Path(assets)
    for sub in ("ships", "geometry", "weapons"):
        (assets / "data" / sub).mkdir(parents=True, exist_ok=True)
    guns, bullets = {}, {}
    for f in sorted(assets.glob("Ship_*.json")):
        name, j = f.stem, json.loads(f.read_text())
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
        ship["sections"] = {
            s["label"]: {
                k: v for k, v in s.items() if k not in ("label", "aabb", "tris")
            }
            for s in j["sections"]
        }
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
        (assets / "data/ships" / f"{name}.json").write_text(dump(rnd(ship)))
        (assets / "data/geometry" / f"{name}.json").write_text(
            json.dumps(geometry, separators=(",", ":"))
        )
        f.unlink()
    (assets / "data/weapons/guns.json").write_text(dump(rnd(guns)))
    (assets / "data/weapons/bullets.json").write_text(dump(rnd(bullets)))


if __name__ == "__main__":
    pack(sys.argv[1])
