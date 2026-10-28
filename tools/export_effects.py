"""Export the effects a set of ships refers to (plus extras) as JSON, with their textures converted to PNG.

usage: python -m tools.export_effects <game_root> <outdir> <ship.json> [...]   (outdir/fx/<name>.json, outdir/fx/tex/*.png)
"""

import json
import sys
from pathlib import Path

from PIL import Image

from tools.eft import load
from tools.export_combat import fx_names
from tools.gamefs import ci_file, ci_files
from tools.mdb_to_glb import convert
from tools.models import model_path

# Effects the game spawns by name, not named by any ship/gun/bullet.
EXTRAS = [
    "Damaged Section_Light",
    "Damaged Section_Medium",
    "Damaged Section_Heavy",
    "Damaged Section_Destroyed",
    "Sparks_Light",
    "Sparks_Impact",
    "Metal_Sparks",
    "FireOnShip",
    "Final_Ship_Fire",
    "Explosion_Medium",
    "Wake_Medium",
    "BlackHoleRibbons_Small",
    "BlackHoleRibbons_Medium",
    "BlackHoleRibbons_Large",
    "Wake_Hole",
    "ShipExplosion_Medium",
    "Explosion_Huge",
    "Nebula",
    "SolarStorm",
    "MeteorStorm",
    "Lightning",
    "Docking_Point",
    "Repair_Small",
    "Repair_Medium",
    "Repair_Large",
    "Repair_IC_Small",
    "Repair_IC_Medium",
    "Repair_IC_Large",
    "Repair_Tender",
    "Boarding_Fight",
    "GrapplingHarpoon_Light_Trail",
    "Rope",  # grapple and tow lines
    # Mission rule effects (Create/Release Event Effect).
    "MoonFish",
    "BottleRocket_Light_Impact",
    # Etherium currents, cloaking, dark matter, dragons, small-ship explosions.
    "EtheriumRiver",
    "Cloaking",
    "DeCloaking",
    "FullyCloaked",
    "Cloaked_Impact",
    "DarkMatter_Impact",
    "DragonFire_Heavy_Bullet",
    "DragonFire_Heavy_Bullet_Impact",
    "Explosion_Light",
    "SuperGravCharge",
    "SuperGravCharge_Bullet",
    # Ambient mesh-scene effects (PlayEffect / VolcanoSmoke attributes, data/mesh_effects.json): fire stacks on bases, smoke, the
    # lighthouse and buoy lights, the volcanoes. Not every descriptor names a file (Volcano_Effect1-4, lighthouse_light2/3).
    "FireStack",
    "Crater_Smoke",
    "Island_Smoke",
    "SmokeStack_Smoke",
    "Lighthouse",
    "bouy",
    "VolcanoSmoke",
    "volcanoSparks",
    "volcanoMuzzleFlash",
    "volcano_Bullet",
    "volcano_Impact",
]


def textures(node, out):
    if isinstance(node, dict):
        for k, v in node.items():
            if k == "TextureName":
                out |= {v} if isinstance(v, str) else set(v)
            else:
                textures(v, out)
    elif isinstance(node, list):
        for v in node:
            textures(v, out)


def export_morph_meshes(root, outdir, effect):
    """Use the existing GLB converter for authored mesh-morph endpoints."""
    for primitive in effect.get("Primitive", []):
        mesh = primitive.get("CommonPropertiesChunk", {}).get("MeshChunk", {})
        if not mesh.get("UseMorph"):
            continue
        for key in ("SourceMeshName", "DestinationMeshName"):
            name = mesh[key]
            dst = model_path(outdir, name, mkdir=True)
            dst.write_bytes(
                convert(
                    root, name, files=[ci_file(Path(root) / "BD_Meshes", name + ".mdb")]
                )
            )


def main(root, outdir, ship_jsons):
    fx = Path(outdir) / "fx"
    (fx / "tex").mkdir(parents=True, exist_ok=True)
    names = set(EXTRAS)
    for p in ship_jsons:
        names |= fx_names(json.loads(Path(p).read_text()))
    done, missing = [], []
    for n in sorted(names):
        try:
            j = load(root, n)
        except FileNotFoundError:
            missing.append(n)
            continue
        export_morph_meshes(root, outdir, j)
        (fx / f"{n}.json").write_text(json.dumps(j))
        done.append(n)
        t = set()
        textures(j, t)
        for tex in t:
            dst = fx / "tex" / (Path(tex).stem.lower() + ".png")
            if dst.exists():
                continue
            src = ci_files(Path(root) / "BD_EffectsTextures", tex)
            if not src:
                print("warning: missing effect texture", tex, file=sys.stderr)
                continue
            Image.open(src[0]).convert("RGBA").save(dst)
    (fx / "index.json").write_text(json.dumps(done))
    print(f"exported {len(done)} effects; missing: {missing}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
