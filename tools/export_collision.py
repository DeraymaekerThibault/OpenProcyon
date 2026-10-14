"""Export WOT collision boxes and converted scenery triangles to data/collision.json.

usage: python3 -m tools.export_collision <game_root> <assets_dir>
Mesh model/LOD 0 is provisional; retail selects its collision LOD separately.
"""

import json
import re
import struct
import sys
from pathlib import Path

from tools.gamefs import ci_file
from tools.models import model_path
from tools.pack_data import rnd


def definition(text):
    block = re.search(
        r"Definition String 'COLLISIONDEFINITION'(.*?)(?=Definition String|\Z)",
        text,
        re.S,
    )
    if not block:
        return None
    text = block[1]

    def field(key):
        return re.search(re.escape(key) + r" String '([^']*)'", text)

    detection, response = field("DetectionType"), field("ResponseType")
    if not detection or detection[1] == "Null" or not response or response[1] == "Null":
        return None
    out = dict(detection=detection[1], response=response[1])
    for key, dest in [("MinExtents", "min"), ("MaxExtents", "max")]:
        m = re.search(key + r" Vector3\(([^)]*)\)", text)
        if m:
            x, y, z = map(float, m[1].split(","))
            out[dest] = [x, z, -y]
    if "min" in out:
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
    for mesh in doc["meshes"]:
        for p in mesh["primitives"]:
            assert p.get("mode", 4) == 4
            verts = accessor(p["attributes"]["POSITION"])
            indices = [v[0] for v in accessor(p["indices"])]
            out.extend(
                [
                    [list(verts[i]) for i in indices[j : j + 3]]
                    for j in range(0, len(indices), 3)
                ]
            )
    return out


def export(root, assets):
    root, assets = Path(root), Path(assets)
    scenes = {}
    for f in (assets / "maps").glob("*.json"):
        m = json.loads(f.read_text())
        if isinstance(m, dict):
            scenes.update({o["type"]: o["scene"] for o in m.get("objects", [])})
    types = set(scenes)
    out = {}
    for typ in sorted(types):
        d = definition(
            ci_file(root / "WorldObjects", typ + ".wot").read_text(encoding="latin1")
        )
        if not d:
            continue
        if d["detection"] == "OBBTree":
            tris = triangles(model_path(assets, scenes[typ]).read_bytes())
            points = list(dict.fromkeys(tuple(p) for t in tris for p in t))
            if not points:
                raise ValueError(f"Empty collision mesh: {typ}")
            lookup = {p: i for i, p in enumerate(points)}
            d["vertices"] = [list(p) for p in points]
            d["indices"] = [[lookup[tuple(p)] for p in t] for t in tris]
            d["min"] = [min(p[i] for p in points) for i in range(3)]
            d["max"] = [max(p[i] for p in points) for i in range(3)]
        if "min" not in d:
            raise ValueError(f"No collision box: {typ}")
        out[typ] = d
    (assets / "data/collision.json").write_text(
        json.dumps(rnd(out, 4), separators=(",", ":")) + "\n"
    )
    print(f"Collision: {len(out)} entity types")


if __name__ == "__main__":
    export(*sys.argv[1:3])
