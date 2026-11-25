"""Dock points: assets/data/docks.json = {entity: {points: [{label, pos, yaw, layer}], tender: bool}}.
Dock points are the MSB nodes of kind 'DockPoint' of Ship_*/Base_*/Island_* world objects that declare 'DockPoint' mesh attributes
(the node's own position, in its layer mesh's vertex frame, plus the offset that places that layer in scenery, `with_layer_offset`;
x starboard / y forward / z up; `yaw` = the node's rotation about z, radians). Not the gun-mount rule of `mdb_to_glb.hardpoints`:
adding the parent layer's position throws bases' dock points tens to hundreds of units off their piers (checked against the model
geometry on every base). Matching by
node kind rather than by the attribute's DescriptorName: several WOTs name every dock point 'default'. `tender` is the ship's 'Is Tender':
the original makes only tenders (and every base/island) dockable and repairing by default (docs/re/docking-and-orders.md).
usage: python -m tools.export_docks <game_root> <assets_dir>"""

from pathlib import Path

from tools import cli, importlog
from tools.export_combat import associations, flat, load, placed_objects, scene_name
from tools.jsonio import write
from tools.mdb_to_glb import layer_offsets, with_layer_offset
from tools.msb import parse_scene_file


def export(root, out):
    docks = {}
    for name in placed_objects(root):
        try:
            nodes = load(root, name)
            if not any(associations(nodes, "DockPoint")):
                continue
            scene = scene_name(nodes)
            msb = {n.id: n for n in parse_scene_file(root, scene)}
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
                    # The damage section the point sits on (bases, islands): losing it closes the object's docks.
                    layer=(
                        msb[n.parent].name
                        if n.parent in msb and msb[n.parent].kind == "DamageSection"
                        else None
                    ),
                )
                for n in msb.values()
                if n.kind == "DockPoint"
            ]
        except (KeyError, ValueError, FileNotFoundError, StopIteration) as e:
            importlog.skipped(f"WorldObjects/{name}.wot", f"{type(e).__name__}: {e}")
            continue
        if points:
            docks[name] = dict(
                points=points, tender=bool(flat(nodes).get("Is Tender", False))
            )
    (Path(out) / "data").mkdir(exist_ok=True)
    write(Path(out) / "data/docks.json", docks)
    print("docks:", len(docks), "entities with dock points")


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
