"""Convert one ship's BD_Meshes/<Name>_Layer*.mdb files into a single textured GLB.

usage: python -m tools.mdb_to_glb <game_root> <ShipSceneName> <out.glb> [--model N]
Layers are assumed to share one ship-space frame (checked visually for the Navy Cutter only).
Game frame is x=starboard, y=bow, z=up (from Port_/Starboard_ mount names); axis map is the rotation
(x, y, z) -> (x, z, -y) into glTF y-up. Triangles are written (a, c, b) because the stored order
is clockwise seen from outside. Smooth vertex normals come from the packed angles in each vertex (mdb.normal).
Hardpoints from BD_MeshScenes/<Scene>.msb become empty nodes under "hardpoints" with extras
{kind, label}; yaw is applied about up. shortcut: parent rotations are ignored (all zero in the cutter).
"""

import argparse
import io
import json
import math
import re
import struct
import sys
from pathlib import Path

from PIL import Image

from tools.gamefs import ci_file, ci_files
from tools.mdb import normal, parse
from tools.msb import parse as parse_msb
from tools.msb import parse_scene, parse_scene_file
from tools.skin import build as build_skin


def to_gltf(p):
    return (p[0], p[2], -p[1])


def model_frame(nodes):
    """{node id: place in the model frame}: each node at its parent's place plus its own position minus its pivot (the original's
    rest pose; docs/re/scenery-placement.md "Ship models"). Rotations are ignored (no retail ship layer but the Procyon Refueling
    Station's is rotated). PROVISIONAL: the root node is the origin; its own pivot's effect on the drawn model is not read."""
    by_id = {n.id: n for n in nodes}

    def place(n, depth=0):
        if n.parent not in by_id or depth > 16:
            return (0.0, 0.0, 0.0)
        p = place(by_id[n.parent], depth + 1)
        return tuple(a + b - c for a, b, c in zip(p, n.pos, n.pivot, strict=True))

    return {i: place(n) for i, n in by_id.items()}


def hardpoints(root, scene):
    """The scene's nodes other than its mesh layers, each with its place in the model frame (`model_frame`)."""
    try:
        path = ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb")
    except FileNotFoundError:
        return []
    nodes = parse_msb(path.read_bytes())
    at = model_frame(nodes)
    return [(n, at[n.id]) for n in nodes if n.kind != "DamageSection"]


def scene_layers(root, scene):
    """Layer MDBs named by the scene's MSB nodes, for scenes whose meshes have another prefix
    (vanilla PirateSilverShip uses JohnSilverShip_Layer*, ProcyonDMRefulingStationShip ProcyonDMRefulingbaseShip_Layer*).
    """
    try:
        nodes = parse_scene_file(root, scene)
    except FileNotFoundError:
        return []
    names = dict.fromkeys(
        n.name
        for n in nodes
        if re.search(r"(?<!Damage)_Layer\d+$", n.name, re.IGNORECASE)
    )
    found = [
        f for name in names for f in ci_files(Path(root) / "BD_Meshes", name + ".mdb")
    ]
    return sorted(
        found,
        key=lambda p: int(re.search(r"Layer(\d+)", p.name, re.IGNORECASE).group(1)),
    )


def pad4(b, fill=b"\0"):
    return b + fill * (-len(b) % 4)


def layer_offsets(root, scene):
    """Where each mesh layer of a scenery scene sits relative to the object's origin, in the MDB-vertex frame: the layer node's
    position minus its pivot. {lower-case layer name: (x, y, z)}; empty when the scene has no MSB."""
    try:
        data = ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
    except FileNotFoundError:
        return {}
    return {
        m["name"].lower(): tuple(
            a - b for a, b in zip(m["pos"], m["pivot"], strict=True)
        )
        for m in parse_scene(data)["meshes"]
    }


def with_layer_offset(offsets, nodes, n):
    """A scene point (dock, gun) in its scenery mesh layer's frame moves with that layer: n.pos plus its parent layer's
    offset (layer_offsets). PROVISIONAL: immediate parent only; a point under a sub-layer of a layer misses the outer offset."""
    parent = nodes.get(n.parent)
    off = offsets.get(parent.name.lower(), (0, 0, 0)) if parent else (0, 0, 0)
    return tuple(a + b for a, b in zip(n.pos, off, strict=True))


def scene_files(root, scene):
    """The scene's own layer MDBs, in layer order (no damage variants)."""
    root = Path(root)
    files = sorted(
        ci_files(root / "BD_Meshes", f"{scene}_Layer*.mdb"),
        key=lambda p: int(re.search(r"Layer(\d+)", p.name, re.IGNORECASE).group(1)),
    )
    return files or scene_layers(root, scene)


def convert(root, scene, model=0, files=None, skin=None, offsets=False):
    """offsets: place each layer at its MSB node's position minus pivot, as a node translation (the original's rest pose: ships,
    their debris, islands and bases; without it a layer stays in its MDB vertex frame).
    files: explicit MDB paths (e.g. one gun layer); default is every <scene>_Layer*.mdb plus MSB hardpoints.
    model: the detail level (index into the MDB's models), or a function of the layer's file stem.
    skin: the MSB scene holding the skeleton and clip of a skinned animal mesh (tools/skin.py)."""
    root = Path(root)
    explicit = files is not None
    files = files if explicit else scene_files(root, scene)
    shifts = layer_offsets(root, scene) if offsets else {}
    if not files:
        raise FileNotFoundError(f"no {scene}_Layer*.mdb under {root / 'BD_Meshes'}")
    if not explicit:
        # Retail: a swappable section's mesh "<A>_<B>" swaps to "<A>Damage_<B>" when destroyed.
        # Nodes named *Damage_Layer* are hidden by the game until then.
        for f in list(files):
            a, b = f.stem.split("_", 1)
            files += ci_files(root / "BD_Meshes", f"{a}Damage_{b}.mdb")
    blob, views, accessors, images, textures, materials, meshes, nodes = (
        bytearray(),
        [],
        [],
        [],
        [],
        [],
        [],
        [],
    )
    skins, animations, extra_roots = [], [], []
    mat_index = {}

    def add_view(data, target=None):
        blob.extend(b"\0" * (-len(blob) % 4))
        v = {"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)}
        if target:
            v["target"] = target
        blob.extend(data)
        views.append(v)
        return len(views) - 1

    def material(name):
        if name in mat_index:
            return mat_index[name]
        found = ci_files(root / "BD_Textures", name.rsplit(".", 1)[0] + ".dds")
        path = found[0] if found else root / "BD_Textures" / "missing.dds"
        mat = {
            "name": name,
            "doubleSided": True,
            "pbrMetallicRoughness": {"metallicFactor": 0, "roughnessFactor": 1},
        }
        if path.exists():
            im = Image.open(path).convert("RGBA")
            buf = io.BytesIO()
            im.save(buf, "PNG")
            images.append(
                {"bufferView": add_view(buf.getvalue()), "mimeType": "image/png"}
            )
            textures.append({"source": len(images) - 1})
            mat["pbrMetallicRoughness"]["baseColorTexture"] = {
                "index": len(textures) - 1
            }
            if im.getchannel("A").getextrema()[0] < 255:
                mat["alphaMode"] = "MASK"
        else:
            print(f"warning: missing texture {path.name}", file=sys.stderr)
        if (
            "sail" in name.lower()
        ):  # shortcut: reference sails are translucent; opacity not decoded from the MDB, fixed 0.75, find the real field to replace
            mat["alphaMode"] = "BLEND"
            mat["pbrMetallicRoughness"]["baseColorFactor"] = [1, 1, 1, 0.75]
        if scene.lower().startswith(("skybox", "selection_indicator")):
            mat["extensions"] = {"KHR_materials_unlit": {}}
            if scene.lower().startswith("skybox"):
                mat.pop("alphaMode", None)
            elif "alphaMode" in mat:
                mat["alphaMode"] = "BLEND"
        materials.append(mat)
        mat_index[name] = len(materials) - 1
        return mat_index[name]

    for f in files:
        raw = f.read_bytes()
        mdb = parse(raw)
        level = model(f.stem) if callable(model) else model
        m = mdb.models[min(level, len(mdb.models) - 1)]
        pos = b"".join(struct.pack("<3f", *to_gltf(v.pos)) for v in m.vertices)
        # Sky spheres keep their authored UVs too: the original draws them unchanged around the camera (docs/re/sky.md), dark poles included.
        uv = b"".join(struct.pack("<2f", *v.uv) for v in m.vertices)
        xs = list(zip(*[to_gltf(v.pos) for v in m.vertices], strict=True))
        accessors.append(
            {
                "bufferView": add_view(pos, 34962),
                "componentType": 5126,
                "count": len(m.vertices),
                "type": "VEC3",
                "min": [min(a) for a in xs],
                "max": [max(a) for a in xs],
            }
        )
        pa = len(accessors) - 1
        accessors.append(
            {
                "bufferView": add_view(uv, 34962),
                "componentType": 5126,
                "count": len(m.vertices),
                "type": "VEC2",
            }
        )
        ua = len(accessors) - 1
        nrm = b"".join(struct.pack("<3f", *to_gltf(normal(v))) for v in m.vertices)
        accessors.append(
            {
                "bufferView": add_view(nrm, 34962),
                "componentType": 5126,
                "count": len(m.vertices),
                "type": "VEC3",
            }
        )
        na = len(accessors) - 1
        by_mat = {}
        for a, b, c, mi in m.faces:
            by_mat.setdefault(mi, []).extend((a, c, b))
        prims = []
        for mi, idx in sorted(by_mat.items()):
            if mi >= len(mdb.textures):
                raise ValueError(
                    f"{f.name}: material index {mi} >= {len(mdb.textures)}"
                )
            accessors.append(
                {
                    "bufferView": add_view(struct.pack(f"<{len(idx)}H", *idx), 34963),
                    "componentType": 5123,
                    "count": len(idx),
                    "type": "SCALAR",
                }
            )
            prims.append(
                {
                    "attributes": {"POSITION": pa, "NORMAL": na, "TEXCOORD_0": ua},
                    "indices": len(accessors) - 1,
                    "material": material(mdb.textures[mi]),
                    "mode": 4,
                }
            )
        meshes.append({"name": f.stem, "primitives": prims})
        mesh_node = len(nodes)
        nodes.append({"name": f.stem, "mesh": len(meshes) - 1})
        # A destroyed section's "<A>Damage_<B>" mesh replaces "<A>_<B>" on the same scene node, so it takes that node's place.
        shift = shifts.get(f.stem.lower()) or shifts.get(
            f.stem.lower().replace("damage_", "_", 1)
        )
        if any(shift or ()):
            nodes[-1]["translation"] = list(to_gltf(shift))
        skinned = skin and build_skin(
            root, skin, raw, len(m.vertices), add_view, accessors, nodes
        )
        if skinned:
            attrs, sk, anims, skeleton = skinned
            for prim in prims:
                prim["attributes"].update(attrs)
            nodes[mesh_node]["skin"] = len(skins)
            skins.append(sk)
            animations += anims
            extra_roots.append(skeleton)

    first_hp = len(nodes)
    for n, pos in [] if explicit else hardpoints(root, scene):
        h = math.sin(n.rot[0] / 2), math.cos(n.rot[0] / 2)
        nodes.append(
            {
                "name": n.label or n.name,
                "translation": list(to_gltf(pos)),
                "rotation": [0, h[0], 0, h[1]],
                "extras": {"kind": n.kind, "node": n.name, "rot_raw": list(n.rot)},
            }
        )
    hp = [i for i in range(first_hp, len(nodes)) if "extras" in nodes[i]]
    nodes.append({"name": "hardpoints", "children": hp})
    gltf = {
        "asset": {"version": "2.0", "generator": "tools.mdb_to_glb"},
        "scene": 0,
        "extensionsUsed": (
            ["KHR_materials_unlit"]
            if scene.lower().startswith(("skybox", "selection_indicator"))
            else []
        ),
        "scenes": [
            {
                "nodes": [
                    *(i for i, n in enumerate(nodes[:first_hp]) if "mesh" in n),
                    *extra_roots,
                    len(nodes) - 1,
                ]
            }
        ],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "textures": textures,
        "images": images,
        "accessors": accessors,
        **({"skins": skins} if skins else {}),
        **({"animations": animations} if animations else {}),
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = pad4(json.dumps(gltf, separators=(",", ":")).encode(), b" ")
    bin_ = pad4(bytes(blob))
    total = 12 + 8 + len(js) + 8 + len(bin_)
    return (
        struct.pack("<4sII", b"glTF", 2, total)
        + struct.pack("<I4s", len(js), b"JSON")
        + js
        + struct.pack("<I4s", len(bin_), b"BIN\0")
        + bin_
    )


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("usage:")[0])
    ap.add_argument("root")
    ap.add_argument("scene")
    ap.add_argument("out")
    ap.add_argument("--model", type=int, default=0)
    ap.add_argument(
        "--offsets",
        action="store_true",
        help="place layers at position minus pivot (ships)",
    )
    a = ap.parse_args()
    Path(a.out).write_bytes(convert(a.root, a.scene, a.model, offsets=a.offsets))
    print(f"wrote {a.out}")
