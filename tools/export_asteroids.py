"""Export ordinary asteroid definitions and missing models; armed asteroids remain ships.
usage: python3 -m tools.export_asteroids <game_root> <assets_dir>
"""

import math
import sys
from pathlib import Path

from tools.export_maps import convert_scene
from tools.gamefs import ci_file, ci_files
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.pack_data import dump
from tools.wot import parse_text, properties


def definition(text):
    nodes, _ = parse_text(text)
    values = {}
    for n in properties(nodes):
        values.setdefault(n["name"], n)
    if not any(
        n["name"] == "FactoryType" and n["value"] == "AsteroidCustomInfoFactory"
        for n in properties(nodes)
    ):
        return None

    def string(key):
        field = values.get(key, {})
        if field.get("type") != "String":
            raise ValueError(f"Missing asteroid field: {key}")
        return field["value"]

    def number(key, kind):
        field = values.get(key, {})
        value = float(field["value"]) if field.get("type") == kind else float("nan")
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"Invalid asteroid field: {key}")
        return value

    return dict(
        scene=string("Mesh Scene Name"),
        hp=number("Max Hit Points", "Int"),
        mass=number("Mass", "Float"),
        radius=number("Radius", "Float"),
        children=[string(f"WorldObject To Create Upon Death {i}") for i in range(10)],
        effect=string("ExplosionEffect"),
    )


def validate(defs):
    def visit(name, ancestors):
        if name not in defs or name in ancestors:
            raise ValueError(f"Invalid asteroid breakup chain: {name}")
        for child in defs[name]["children"]:
            if child:
                visit(child, ancestors | {name})

    for name in defs:
        visit(name, set())


def export(root, assets):
    root, assets = Path(root), Path(assets)
    defs = {}
    for path in ci_files(root / "WorldObjects", "Asteroid*.wot"):
        d = definition(path.read_text(encoding="latin1"))
        if d:
            defs[path.stem] = d
    if not defs:
        raise ValueError("No ordinary asteroid definitions found")
    validate(defs)
    for d in defs.values():
        dst = model_path(assets, d["scene"], mkdir=True)
        if not dst.exists():
            scene = d["scene"]
            # These MSBs use layer names that differ from their scene names.
            layers = {
                f"AsteroidDarkmatter{i}": f"ASTEROIDSDarkmatter{i}" for i in range(1, 5)
            }
            layers["AsteroidXlarge"] = "ASTEROIDS_XLarge"
            layer = layers.get(scene)
            dst.write_bytes(
                convert(
                    root,
                    scene,
                    files=[ci_file(root / "BD_Meshes", f"{layer}_Layer1.mdb")],
                )
                if layer
                else convert_scene(root, scene)
            )
    (assets / "data").mkdir(exist_ok=True)
    (assets / "data/asteroids.json").write_text(dump(defs))
    print(f"exported {len(defs)} ordinary asteroid definitions")


if __name__ == "__main__":
    export(*sys.argv[1:])
