"""Damage decals (BD_DecalTextures/*.tga) -> assets/decals/<name>.png plus data/decals.json {name: {size, materials}}.

The original hard-codes this list and files each texture under size class 0/1/2 from its _Small/_Medium/_Large suffix
and under surface groups (wood and reinforced wood share one; iron, cloth, stone, dragon). The groups below reproduce its
table: a few textures go in every group or in wood and cloth, the rest by name; engine_part, rudder and hole_damage60 are
never registered, so they are skipped.
usage: python3 -m tools.export_decals <game_root> <assets_dir>
"""

import json
import sys
from pathlib import Path

from PIL import Image

SIZES = {"Small": 0, "Medium": 1, "Large": 2}


# Textures the original registers for every surface, and for wood plus cloth (decoded); anything else is filed by its name.
EVERYWHERE = {
    "burn2_small",
    "dent1_large",
    "dent_4_medium",
    "sail_burned1_large",
    "sail_burned2_large",
}
WOOD_AND_CLOTH = {
    "burn1_large",
    "hole5_a_small",
    "dent6_small",
    "dent_1_medium",
    "hole4_medium",
    "hole5_medium",
}
# The original never registers these, so no hit ever draws them.
UNUSED = ("engine_part", "rudder", "hole_damage60")


def materials(full):
    """Surface groups of one texture (file stem with its size suffix); None when the original never uses it."""
    s = full.lower()
    if s.startswith(UNUSED):
        return None
    if s in EVERYWHERE:
        return ["Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon"]
    if s in WOOD_AND_CLOTH:
        return ["Wood", "Reinforced", "Cloth"]
    if s.startswith("sail_"):
        return ["Cloth"]
    if s.startswith("steeldecal"):
        return ["Iron"]
    if s.startswith("wall dmg"):
        return ["Stone"]
    return ["Wood", "Reinforced"]


def main(root, out):
    root, out = Path(root), Path(out)
    (out / "decals").mkdir(parents=True, exist_ok=True)
    table = {}
    for f in sorted((root / "BD_DecalTextures").glob("*.tga")):
        size = f.stem.rpartition("_")[2]
        if size not in SIZES:
            continue
        groups = materials(f.stem)
        if groups is None:
            continue
        name = f.stem.lower().replace(" ", "_")
        Image.open(f).convert("RGBA").save(out / "decals" / f"{name}.png")
        table[name] = dict(size=SIZES[size], materials=groups)
    (out / "data").mkdir(exist_ok=True)
    (out / "data/decals.json").write_text(json.dumps(table, indent=1) + "\n")
    print(f"decals: {len(table)}")


if __name__ == "__main__":
    main(*sys.argv[1:3])
