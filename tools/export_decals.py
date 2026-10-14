"""Damage decals (BD_DecalTextures/*.tga) -> assets/decals/<name>.png plus data/decals.json {name: {size, materials}}.

The exe hard-codes this list and files each texture under size class 0/1/2 from its _Small/_Medium/_Large suffix
and under material buckets by five flag bits (wood, reinforced, iron, sail, stone) that are not decoded yet.
PROVISIONAL: the material buckets below are read from the file names.
usage: python3 -m tools.export_decals <game_root> <assets_dir>
"""

import json
import sys
from pathlib import Path

from PIL import Image

SIZES = {"Small": 0, "Medium": 1, "Large": 2}


def materials(stem):
    s = stem.lower()
    if s.startswith("sail_"):
        return ["Cloth"]
    if s.startswith("steeldecal"):
        return ["Iron"]
    if s.startswith("wall dmg"):
        return ["Stone"]
    if "dent" in s or s.startswith(("engine_part", "rudder")):
        return ["Iron", "Reinforced"]
    return ["Wood", "Reinforced"]


def main(root, out):
    root, out = Path(root), Path(out)
    (out / "decals").mkdir(parents=True, exist_ok=True)
    table = {}
    for f in sorted((root / "BD_DecalTextures").glob("*.tga")):
        stem, _, size = f.stem.rpartition("_")
        if size not in SIZES:
            continue
        name = f.stem.lower().replace(" ", "_")
        Image.open(f).convert("RGBA").save(out / "decals" / f"{name}.png")
        table[name] = dict(size=SIZES[size], materials=materials(stem))
    (out / "data").mkdir(exist_ok=True)
    (out / "data/decals.json").write_text(json.dumps(table, indent=1) + "\n")
    print(f"decals: {len(table)}")


if __name__ == "__main__":
    main(*sys.argv[1:3])
