"""Convert one ship's BD_Meshes/<Name>_Layer*.mdb files into a single textured GLB.

usage: python -m tools.mdb_to_glb <game_root> <ShipSceneName> <out.glb> [--model N]
Layers are assumed to share one ship-space frame (checked visually for the Navy Cutter only).
Game frame is x=starboard, y=bow, z=up (from Port_/Starboard_ mount names); axis map is the rotation
(x, y, z) -> (x, z, -y) into glTF y-up. Triangles are written (a, c, b) because the stored order
is clockwise seen from outside. Smooth vertex normals come from the packed angles in each vertex (mdb.normal).
Hardpoints from BD_MeshScenes/<Scene>.msb become empty nodes under "hardpoints" with extras
{kind, label}; yaw is applied about up. shortcut: parent rotations are ignored (all zero in the cutter).
"""

import io
import json
import re
import struct
import sys
from pathlib import Path

from PIL import Image

import math

from tools.mdb import ci_file, ci_files, normal, parse
from tools.msb import parse as parse_msb


def to_gltf(p):
    return (p[0], p[2], -p[1])


def hardpoints(root, scene):
    """Nodes in MDB-vertex frame: translation-only parent chain, minus the common layer offset."""
    try:
        path = ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb")
    except FileNotFoundError:
        return []
    nodes = {n.id: n for n in parse_msb(path.read_bytes())}

    def world(n, depth=0):
        if n.parent not in nodes or depth > 16:
            return n.pos
        w = world(nodes[n.parent], depth + 1)
        return tuple(a + b for a, b in zip(w, n.pos))

    layer = [world(n) for n in nodes.values() if n.kind == "DamageSection"]
    off = layer[0] if layer else (0, 0, 0)
    return [
        (n, tuple(a - b for a, b in zip(world(n), off)))
        for n in nodes.values()
        if n.kind != "DamageSection"
    ]


def scene_layers(root, scene):
    """Layer MDBs named by the scene's MSB nodes, for scenes whose meshes have another prefix
    (vanilla PirateSilverShip uses JohnSilverShip_Layer*, ProcyonDMRefulingStationShip ProcyonDMRefulingbaseShip_Layer*)."""
    try:
        nodes = parse_msb(ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes())
    except FileNotFoundError:
        return []
    names = dict.fromkeys(n.name for n in nodes if re.search(r"(?<!Damage)_Layer\d+$", n.name, re.I))
    found = [f for name in names for f in ci_files(Path(root) / "BD_Meshes", name + ".mdb")]
    return sorted(found, key=lambda p: int(re.search(r"Layer(\d+)", p.name, re.I).group(1)))


def pad4(b, fill=b"\0"):
    return b + fill * (-len(b) % 4)


def convert(root, scene, model=0, files=None):
    """files: explicit MDB paths (e.g. one gun layer); default is every <scene>_Layer*.mdb plus MSB hardpoints."""
    root = Path(root)
    explicit = files is not None
    files = (
        files
        if explicit
        else sorted(
            ci_files(root / "BD_Meshes", f"{scene}_Layer*.mdb"),
            key=lambda p: int(re.search(r"Layer(\d+)", p.name, re.I).group(1)),
        )
    )
    if not files and not explicit:
        files = scene_layers(root, scene)
    if not files:
        raise FileNotFoundError(f"no {scene}_Layer*.mdb under {root / 'BD_Meshes'}")
    if not explicit:
        # Exe (section setup): a swappable section's mesh "<A>_<B>" swaps to "<A>Damage_<B>" when destroyed.
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
        mdb = parse(f.read_bytes())
        m = mdb.models[model]
        pos = b"".join(struct.pack("<3f", *to_gltf(v.pos)) for v in m.vertices)
        # Skybox art is dark above/below its bright band (v<0.2, v>0.8; it keeps 0.3..0.7) and the sphere maps the whole texture; squeeze the sphere's
        # v range (0.127..0.875) into the bright band so no direction looks black. shortcut: not what the retail renderer does if it clamps the camera pitch.
        sky = scene.lower().startswith("skybox")
        uv = b"".join(
            struct.pack(
                "<2f",
                v.uv[0],
                0.3 + (v.uv[1] - 0.127) / 0.748 * 0.4 if sky else v.uv[1],
            )
            for v in m.vertices
        )
        xs = list(zip(*[to_gltf(v.pos) for v in m.vertices]))
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
        nodes.append({"name": f.stem, "mesh": len(meshes) - 1})

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
    hp = list(range(first_hp, len(nodes)))
    nodes.append({"name": "hardpoints", "children": hp})
    gltf = {
        "asset": {"version": "2.0", "generator": "tools.mdb_to_glb"},
        "scene": 0,
        "extensionsUsed": ["KHR_materials_unlit"]
        if scene.lower().startswith(("skybox", "selection_indicator"))
        else [],
        "scenes": [{"nodes": list(range(first_hp)) + [len(nodes) - 1]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "textures": textures,
        "images": images,
        "accessors": accessors,
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
    args = sys.argv[1:]
    model = int(args[args.index("--model") + 1]) if "--model" in args else 0
    root, scene, out = args[:3]
    Path(out).write_bytes(convert(root, scene, model))
    print(f"wrote {out}")
