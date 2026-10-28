"""Ambient effect points of mesh scenes: assets/data/mesh_effects.json = {scene: [{kind, name, descriptor, pos}]} for the MSB nodes of
kind PlayEffect or VolcanoSmoke (fire stacks, smoke, lighthouse beams, volcano fountains, buoy light). `pos` is the node's own
position in the scene's frame (x starboard, y forward, z up); `descriptor` names the effect ('default' leaves it to the node name).
The effect files are not exported yet. msb.py reads the first attribute of a node only, so a node that also carries a second
attribute (one volcano smoke node) shows that one only.
usage: python -m tools.export_mesh_effects <game_root> <assets_dir>"""

import json
import sys
from pathlib import Path

from tools.gamefs import ci_files
from tools.msb import parse

KINDS = ("PlayEffect", "VolcanoSmoke")


def export(root, out):
    scenes = {}
    for msb in ci_files(Path(root) / "BD_MeshScenes", "*.msb"):
        points = [
            dict(
                kind=n.kind,
                name=n.name,
                descriptor=n.label,
                pos=[round(c, 3) for c in n.pos],
            )
            for n in parse(msb.read_bytes())
            if n.kind in KINDS
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
