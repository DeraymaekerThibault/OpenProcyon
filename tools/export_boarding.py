"""Boarding data: assets/data/boarding.json = {entity: {points: [[x, y, z]], crew: [8 counts]}}.
`points` are the MSB nodes of kind 'BoardingEffectPoint' (object-local game frame) of every Ship_/Base_/Island_ that has them: where the
boarding fight effect plays and, for bases, what the boarder must be near. `crew` (ships only) are the WOT 'Max number of ...' crew counts
in the exe's role order Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts (docs/re/boarding.md).
usage: python -m tools.export_boarding <game_root> <assets_dir>"""

import sys
from pathlib import Path

from tools.export_combat import flat, load, scene_name
from tools.gamefs import ci_files
from tools.mdb_to_glb import hardpoints
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


def export(root, out):
    data = {}
    for wot in ci_files(Path(root) / "WorldObjects", "*.wot"):
        name = wot.stem
        if not name.startswith(("Ship_", "Base_", "Island_")):
            continue
        try:
            nodes = load(root, name)
            points = [
                [round(c, 3) for c in p]
                for n, p in hardpoints(root, scene_name(nodes))
                if n.kind == "BoardingEffectPoint"
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
