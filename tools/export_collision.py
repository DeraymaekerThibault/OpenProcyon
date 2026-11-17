"""Export WOT collision boxes and converted scenery triangles to data/collision.json.

usage: python3 -m tools.export_collision <game_root> <assets_dir>
Triangles come from each layer's collision detail level (MeshDetailSettings 'Collision LOD Level Index'), placed where the layer
is drawn (its scene node's position minus pivot).

Entries with OBBTree detection (islands, bases) also carry the mesh files' tree of oriented boxes, for ships touching land:
  "layers": [{"offset": [x, y, z],   # where the layer sits relative to the object's origin (already in the triangles above)
              "nodes": [{"c": [x, y, z],            # box centre, in the layer's frame
                         "a": [[x, y, z] x 3],      # the three unit axes
                         "h": [hx, hy, hz],         # half extents along those axes
                         "l": level,                # root = 0
                         "k": [child node indices], # at most two
                         "t": [triangle indices]}]}]  # nodes are in pre-order, node 0 is the root
All vectors are in remake axes (x, up, -y of the game frame), like the triangles.
"""

import json
import struct
import sys
from pathlib import Path

from tools.export_mesh_detail import collision_lods
from tools.gamefs import ci_file
from tools.mdb import box_tree, parse
from tools.mdb_to_glb import convert, layer_offsets, scene_files
from tools.pack_data import rnd
from tools.wot import fields, parse_text, properties, records


def definition(text):
    nodes, _ = parse_text(text)

    def find(children):
        for record in records(children, "Definition"):
            f = fields(properties(record))
            if f["Definition"] == "COLLISIONDEFINITION":
                return f
        for node in children:
            if node["kind"] == "block":
                found = find(node["children"])
                if found is not None:
                    return found
        return None

    f = find(nodes)
    if (
        not f
        or f.get("DetectionType", "Null") == "Null"
        or f.get("ResponseType", "Null") == "Null"
    ):
        return None
    out = dict(detection=f["DetectionType"], response=f["ResponseType"])
    for key, dest in [("MinExtents", "min"), ("MaxExtents", "max")]:
        if key in f:
            x, y, z = f[key]
            out[dest] = [x, z, -y]
    if "min" in out or "max" in out:
        if "min" not in out or "max" not in out:
            raise ValueError("Incomplete collision box")
        out["min"][2], out["max"][2] = out["max"][2], out["min"][2]
    return out


def triangles(blob):
    """Read the indexed triangle primitives written by our GLB converter (no node transforms)."""
    length, kind = struct.unpack_from("<II", blob, 12)
    assert kind == 0x4E4F534A
    doc = json.loads(blob[20 : 20 + length])
    binary = blob[28 + length :]

    def accessor(index):
        a = doc["accessors"][index]
        v = doc["bufferViews"][a["bufferView"]]
        fmt = {5123: "H", 5125: "I", 5126: "f"}[a["componentType"]]
        width = {"SCALAR": 1, "VEC3": 3}[a["type"]]
        start = v.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = v.get("byteStride", struct.calcsize("<" + fmt * width))
        return [
            struct.unpack_from("<" + fmt * width, binary, start + i * stride)
            for i in range(a["count"])
        ]

    out = []
    # Mesh nodes may carry a translation (the layer offset); without nodes every mesh sits at the origin.
    placed = [
        (n["mesh"], n.get("translation", [0, 0, 0]))
        for n in doc.get("nodes", [])
        if "mesh" in n
    ] or [(i, [0, 0, 0]) for i in range(len(doc["meshes"]))]
    for mesh_index, shift in placed:
        for p in doc["meshes"][mesh_index]["primitives"]:
            assert p.get("mode", 4) == 4
            verts = [
                tuple(c + d for c, d in zip(v, shift, strict=True))
                for v in accessor(p["attributes"]["POSITION"])
            ]
            indices = [v[0] for v in accessor(p["indices"])]
            out.extend(
                [
                    [list(verts[i]) for i in indices[j : j + 3]]
                    for j in range(0, len(indices), 3)
                ]
            )
    return out


def bevy(v):
    """Game axes (x, y, z up) to remake axes: the rotation (x, y, z) -> (x, z, -y)."""
    return [v[0], v[2], -v[1]]


def flat_tree(root_node):
    """The nested tree from `tools.mdb.box_tree` as the flat pre-order node list of the collision data, in remake axes."""
    nodes = []

    def add(n):
        i = len(nodes)
        nodes.append(
            {
                "c": bevy(n["centre"]),
                "a": [bevy(a) for a in n["axes"]],
                "h": list(n["half"]),
                "l": n["level"],
                "k": [],
                "t": n["tris"],
            }
        )
        nodes[i]["k"] = [add(c) for c in n["children"]]
        return i

    add(root_node)
    return nodes


def layers(root, scene):
    """Box tree and offset of each of the scene's own layers that has a tree."""
    shifts = layer_offsets(root, scene)
    out = []
    for f in scene_files(root, scene):
        tree = box_tree(parse(f.read_bytes()).trailing)
        if tree:
            off = shifts.get(f.stem.lower(), (0.0, 0.0, 0.0))
            out.append({"offset": bevy(off), "nodes": flat_tree(tree)})
    return out


def export(root, assets):
    root, assets = Path(root), Path(assets)
    scenes = {}
    # maps/<id>.json right after the map export, maps/<id>/map.json once the import has regrouped the folders
    for f in [*(assets / "maps").glob("*.json"), *(assets / "maps").glob("*/map.json")]:
        m = json.loads(f.read_text())
        if isinstance(m, dict):
            scenes.update({o["type"]: o["scene"] for o in m.get("objects", [])})
    types = set(scenes)
    lod = collision_lods(root)
    out = {}
    for typ in sorted(types):
        d = definition(
            ci_file(root / "WorldObjects", typ + ".wot").read_text(encoding="latin1")
        )
        if not d:
            continue
        if d["detection"] == "OBBTree":
            tris = triangles(convert(root, scenes[typ], model=lod, offsets=True))
            points = list(dict.fromkeys(tuple(p) for t in tris for p in t))
            if not points:
                raise ValueError(f"Empty collision mesh: {typ}")
            lookup = {p: i for i, p in enumerate(points)}
            d["vertices"] = [list(p) for p in points]
            d["indices"] = [[lookup[tuple(p)] for p in t] for t in tris]
            d["min"] = [min(p[i] for p in points) for i in range(3)]
            d["max"] = [max(p[i] for p in points) for i in range(3)]
            d["layers"] = layers(root, scenes[typ])
        if "min" not in d:
            raise ValueError(f"No collision box: {typ}")
        out[typ] = d
    (assets / "data/collision.json").write_text(
        json.dumps(rnd(out, 4), separators=(",", ":")) + "\n"
    )
    print(f"Collision: {len(out)} entity types")


if __name__ == "__main__":
    export(*sys.argv[1:3])
