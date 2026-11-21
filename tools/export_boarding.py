"""Boarding data: assets/data/boarding.json = {entity: {points: [[x, y, z]], crew: [8 counts]}}.
`points` are the MSB nodes of kind 'BoardingEffectPoint' (object-local game frame) of every Ship_/Base_/Island_ that has them: where the
boarding fight effect plays and, for bases, what the boarder must be near. A base's or island's point is the node's own position moved
with its mesh layer (`with_layer_offset`, the dock rule); a ship's comes from `hardpoints`. `crew` (ships only) are the WOT 'Max number of ...' crew counts
in the original's role order Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts (docs/re/boarding.md).
usage: python -m tools.export_boarding <game_root> <assets_dir>"""

import sys
from pathlib import Path

from tools.export_combat import flat, load, placed_objects, scene_name
from tools.gamefs import ci_file
from tools.mdb_to_glb import hardpoints, layer_offsets, with_layer_offset
from tools.msb import parse as parse_msb
from tools.pack_data import dump

ROLES = (
    "Gunners",
    "Captains",
    "First Mates",
    "Navigators",
    "Engineers",
    "Riggers",
    "Fighters",
    "Lookouts",
)


def scenery_points(root, scene):
    """Boarding points of a base or island, in the frame its placed meshes and docks share."""
    msb = {
        n.id: n
        for n in parse_msb(
            ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
        )
    }
    offsets = layer_offsets(root, scene)
    return [
        with_layer_offset(offsets, msb, n)
        for n in msb.values()
        if n.kind == "BoardingEffectPoint"
    ]


def export(root, out):
    data = {}
    for name in placed_objects(root):
        try:
            nodes = load(root, name)
            scene = scene_name(nodes)
            points = [
                [round(c, 3) for c in p]
                for p in (
                    scenery_points(root, scene)
                    if name.startswith(("Base_", "Island_"))
                    else [
                        p
                        for n, p in hardpoints(root, scene)
                        if n.kind == "BoardingEffectPoint"
                    ]
                )
            ]
        except (KeyError, ValueError, FileNotFoundError, StopIteration):
            continue
        entry = {"points": points}
        if name.startswith("Ship_"):
            f = flat(nodes)
            entry["crew"] = [int(f.get(f"Max number of {r}", 0)) for r in ROLES]
        if points or "crew" in entry:
            data[name] = entry
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/boarding.json").write_text(dump(data))
    print("boarding:", len(data), "entities")


if __name__ == "__main__":
    export(*sys.argv[1:3])
