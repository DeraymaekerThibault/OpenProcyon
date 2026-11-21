"""Dock points: assets/data/docks.json = {entity: {points: [{label, pos, yaw}], tender: bool}}.
Dock points are the MSB nodes of kind 'DockPoint' of Ship_*/Base_*/Island_* world objects that declare 'DockPoint' mesh attributes
(the node's own position, in its layer mesh's vertex frame, plus the offset that places that layer in scenery, `with_layer_offset`;
x starboard / y forward / z up; `yaw` = the node's rotation about z, radians). Not the gun-mount rule of `mdb_to_glb.hardpoints`:
adding the parent layer's position throws bases' dock points tens to hundreds of units off their piers (checked against the model
geometry on every base). Matching by
node kind rather than by the attribute's DescriptorName: several WOTs name every dock point 'default'. `tender` is the ship's 'Is Tender':
the original makes only tenders (and every base/island) dockable and repairing by default (docs/re/docking-and-orders.md).
usage: python -m tools.export_docks <game_root> <assets_dir>"""

import json
import sys
from pathlib import Path

from tools.export_combat import associations, flat, load, placed_objects, scene_name
from tools.gamefs import ci_file
from tools.mdb_to_glb import layer_offsets, with_layer_offset
from tools.msb import parse as parse_msb


def export(root, out):
    docks = {}
    for name in placed_objects(root):
        try:
            nodes = load(root, name)
            if not any(associations(nodes, "DockPoint")):
                continue
            scene = scene_name(nodes)
            msb = {
                n.id: n
                for n in parse_msb(
                    ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
                )
            }
            # Ship meshes are exported without layer offsets, so only scenery points move with their layer.
            offsets = (
                layer_offsets(root, scene)
                if name.startswith(("Base_", "Island_"))
                else {}
            )
            points = [
                dict(
                    label=n.label,
                    pos=[round(c, 3) for c in with_layer_offset(offsets, msb, n)],
                    yaw=round(n.rot[0], 4),
                )
                for n in msb.values()
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
