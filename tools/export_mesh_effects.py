"""Ambient effect points of mesh scenes: assets/data/mesh_effects.json = {scene: [{kind, name, descriptor, pos}]} for the MSB nodes of
kind PlayEffect or VolcanoSmoke (fire stacks, smoke, lighthouse beams, volcano fountains, buoy light). `pos` is the node's own
position in the scene's frame (x starboard, y forward, z up); `descriptor` names the effect ('default' leaves it to the node name).
A node may carry several attributes (one volcano smoke node has both kinds) and gets one entry per attribute.
usage: python -m tools.export_mesh_effects <game_root> <assets_dir>"""

import json
import sys
from pathlib import Path

from tools.gamefs import ci_files
from tools.msb import parse_scene

KINDS = ("PlayEffect", "VolcanoSmoke")


def export(root, out):
    scenes = {}
    for msb in ci_files(Path(root) / "BD_MeshScenes", "*.msb"):
        scene = parse_scene(msb.read_bytes())
        points = [
            dict(
                kind=kind,
                name=el["name"],
                descriptor=descriptor,
                pos=[round(c, 3) for c in el["pos"]],
            )
            for group in ("nodes", "meshes", "bones")
            for el in scene[group]
            for kind, descriptor in el["attributes"]
            if kind in KINDS
        ]
        if points:
            scenes[msb.stem] = points
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/mesh_effects.json").write_text(json.dumps(scenes, indent=1))
    print(
        "mesh effects:",
        sum(map(len, scenes.values())),
        "points in",
        len(scenes),
        "scenes",
    )


if __name__ == "__main__":
    export(*sys.argv[1:3])
