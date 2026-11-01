"""Mesh detail settings: assets/data/mesh_detail.json = {mesh name: {levels: [[max camera distance, detail fraction], ...],
no_draw, collision_lod}} from MeshDetailSettings/MeshDetailSettings.dmt. The entry with an empty name is the default.
`collision_lod` is the index of the level whose triangles retail collides with (docs/re/collision.md).
usage: python -m tools.export_mesh_detail <game_root> <assets_dir>"""

import functools
import json
import re
import sys
from pathlib import Path

from tools.gamefs import ci_file


def parse(text):
    meshes, cur = {}, None
    for line in text.splitlines():
        line = line.strip()
        if m := re.match(r"Mesh Name String '(.*)'", line):
            cur = meshes.setdefault(
                m.group(1), dict(levels=[], no_draw=0.0, collision_lod=0)
            )
            dist = None
        elif cur is None:
            continue
        elif m := re.match(r"Max Camera Distance Float (\S+)", line):
            dist = float(m.group(1))
        elif m := re.match(r"Mesh Detail Percentage Float (\S+)", line):
            cur["levels"].append([dist, float(m.group(1))])
        elif m := re.match(r"Camera Distance To Not Draw Mesh Float (\S+)", line):
            cur["no_draw"] = float(m.group(1))
        elif m := re.match(r"Collision LOD Level Index Int (\d+)", line):
            cur["collision_lod"] = int(m.group(1))
    return meshes


def lod_lookup(meshes):
    """Function mesh name (any case) -> index of the model (detail level) retail collides with; unlisted meshes use the default entry."""
    default = meshes.get("", {}).get("collision_lod", 0)
    lower = {k.lower(): v["collision_lod"] for k, v in meshes.items()}
    return lambda name: lower.get(name.lower(), default)


@functools.cache
def _lods(root):
    src = ci_file(Path(root) / "MeshDetailSettings", "MeshDetailSettings.dmt")
    return lod_lookup(parse(src.read_text(encoding="latin1")))


def collision_lods(root):
    return _lods(str(root))


def export(root, out):
    src = ci_file(Path(root) / "MeshDetailSettings", "MeshDetailSettings.dmt")
    meshes = parse(src.read_text(encoding="latin1"))
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/mesh_detail.json").write_text(json.dumps(meshes, indent=0))
    print("mesh detail:", len(meshes), "meshes")


if __name__ == "__main__":
    export(*sys.argv[1:3])
