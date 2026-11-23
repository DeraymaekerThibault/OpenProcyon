"""Ambient effect points of mesh scenes: assets/data/mesh_effects.json = {scene: [{kind, name, descriptor, pos}]} for the MSB nodes of
kind PlayEffect or VolcanoSmoke (fire stacks, smoke, lighthouse beams, volcano fountains, buoy light). `pos` is the node's own
position moved with its mesh layer (`with_layer_offset`, as docks; x starboard, y forward, z up); `descriptor` names the effect ('default' leaves it to the node name).
A node may carry several attributes (one volcano smoke node has both kinds) and gets one entry per attribute.
`effect` is the effect file the world object names for the point (None for VolcanoSmoke points).
usage: python -m tools.export_mesh_effects <game_root> <assets_dir>"""

import json
import re
import struct
from pathlib import Path

from tools import cli
from tools.gamefs import ci_files
from tools.mdb_to_glb import with_layer_offset
from tools.msb import parse as parse_msb
from tools.msb import parse_scene

KINDS = ("PlayEffect", "VolcanoSmoke")
ATTRIBUTE = re.compile(
    r"AttributeName String 'PlayEffect'\s*DescriptorName String '([^']*)'\s*AssociationName String '([^']*)'"
)


def associations(root):
    """{(mesh scene, node name): effect file name} from the world objects: a PlayEffect attribute's AssociationName names the
    effect the point plays (the descriptor only labels the point)."""
    found = {}
    for wot in ci_files(Path(root) / "WorldObjects", "*.wot"):
        text = wot.read_text(encoding="latin1")
        scene = re.search(r"Mesh Scene Name String '([^']*)'", text)
        if not scene:
            continue
        for block in re.finditer(
            r"MeshName String '([^']*)'(.*?)(?=MeshName String|\Z)", text, re.DOTALL
        ):
            attribute = ATTRIBUTE.search(block.group(2))
            if attribute:
                found[(scene.group(1), block.group(1))] = attribute.group(2)
    return found


def export(root, out):
    scenes = {}
    effects = associations(root)
    for msb in ci_files(Path(root) / "BD_MeshScenes", "*.msb"):
        try:
            data = msb.read_bytes()
            scene = parse_scene(data)
            nodes = {n.id: n for n in parse_msb(data)}
        except (ValueError, struct.error) as e:
            print(
                f"{msb.name}: skipped ({e})"
            )  # unreadable (modded) scene: no effect points
            continue
        # The lighthouse beam hangs under a layer 8.6 below the object; without its offset it floated above the tower.
        offsets = {
            m["name"].lower(): tuple(
                a - b for a, b in zip(m["pos"], m["pivot"], strict=True)
            )
            for m in scene["meshes"]
        }
        placed = {n.name: with_layer_offset(offsets, nodes, n) for n in nodes.values()}
        points = [
            dict(
                kind=kind,
                name=el["name"],
                descriptor=descriptor,
                effect=effects.get((msb.stem, el["name"]))
                if kind == "PlayEffect"
                else None,
                pos=[round(c, 3) for c in placed.get(el["name"], el["pos"])],
            )
            for group in ("nodes", "meshes", "bones")
            for el in scene[group]
            for kind, descriptor in el["attributes"]
            if kind in KINDS
        ]
        if points:
            scenes[msb.stem] = points
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/mesh_effects.json").write_text(
        json.dumps(scenes, indent=1), encoding="utf-8"
    )
    print(
        "mesh effects:",
        sum(map(len, scenes.values())),
        "points in",
        len(scenes),
        "scenes",
    )


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
