"""Boarding data: assets/data/boarding.json = {entity: {points: [[x, y, z]], crew: [8 counts]}}.
`points` are the MSB nodes of kind 'BoardingEffectPoint' (object-local game frame) of every Ship_/Base_/Island_ that has them: where the
boarding fight effect plays and, for bases, what the boarder must be near. A base's or island's point is the node's own position moved
with its mesh layer (`with_layer_offset`, the dock rule); a ship's comes from `hardpoints`. `crew` (ships only) are the WOT 'Max number of ...' crew counts
in the original's role order Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts (docs/re/boarding.md).
usage: python -m tools.export_boarding <game_root> <assets_dir>"""

from pathlib import Path

from tools import cli, importlog
from tools.export_combat import flat, load, placed_objects, scene_name
from tools.mdb_to_glb import hardpoints, layer_offsets, with_layer_offset
from tools.msb import parse_scene_file
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
    msb = {n.id: n for n in parse_scene_file(root, scene)}
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
        except (KeyError, ValueError, FileNotFoundError, StopIteration) as e:
            importlog.skipped(f"WorldObjects/{name}.wot", f"{type(e).__name__}: {e}")
            continue
        entry = {"points": points}
        if name.startswith("Ship_"):
            f = flat(nodes)
            entry["crew"] = [int(f.get(f"Max number of {r}", 0)) for r in ROLES]
        if points or "crew" in entry:
            data[name] = entry
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/boarding.json").write_text(dump(data), encoding="utf-8")
    print("boarding:", len(data), "entities")


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
