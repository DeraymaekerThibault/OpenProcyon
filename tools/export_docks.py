"""Dock points: assets/data/docks.json = {entity: {points: [{label, pos, yaw}], tender: bool}}.
Dock points are the MSB nodes of kind 'DockPoint' of Ship_*/Base_*/Island_* world objects that declare 'DockPoint' mesh attributes
(positions in the object's local game frame, x starboard / y forward / z up; `yaw` = the node's rotation about z, radians). Matching by
node kind rather than by the attribute's DescriptorName: several WOTs name every dock point 'default'. `tender` is the ship's 'Is Tender':
the exe makes only tenders (and every base/island) dockable and repairing by default (docs/re/docking-and-orders.md).
usage: python -m tools.export_docks <game_root> <assets_dir>"""

import json
import sys
from pathlib import Path

from tools.export_combat import associations, flat, load, scene_name
from tools.gamefs import ci_files
from tools.mdb_to_glb import hardpoints


def export(root, out):
    docks = {}
    for wot in ci_files(Path(root) / "WorldObjects", "*.wot"):
        name = wot.stem
        if not name.startswith(("Ship_", "Base_", "Island_")):
            continue
        try:
            nodes = load(root, name)
            if not any(associations(nodes, "DockPoint")):
                continue
            points = [
                dict(
                    label=n.label, pos=[round(c, 3) for c in p], yaw=round(n.rot[0], 4)
                )
                for n, p in hardpoints(root, scene_name(nodes))
                if n.kind == "DockPoint"
            ]
        except (KeyError, ValueError, FileNotFoundError, StopIteration):
            continue
        if points:
            docks[name] = dict(
                points=points, tender=bool(flat(nodes).get("Is Tender", False))
            )
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/docks.json").write_text(json.dumps(docks, indent=1))
    print("docks:", len(docks), "entities with dock points")


if __name__ == "__main__":
    export(*sys.argv[1:3])
