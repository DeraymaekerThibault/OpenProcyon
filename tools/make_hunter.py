"""Builds the meshes of mods/hunter (a Procyon sniper trimaran and its railgun turret) as GLB, pure Python, no Blender.

    python3 tools/make_hunter.py mods/hunter

Own art, CC0. Game axes (x starboard, y forward, z up); GLB uses (x, z, -y) with winding (a, c, b).

The surfaces name retail textures (`retail:<texture>`, docs/modding.md "Retail textures in a mod model"): the game fills them in from
the player's own import when it builds the overlay, so no game art is stored here. Each material also carries a flat fallback colour.
The design follows docs/world/design-language.md and factions.md: a slender planked central hull with a raised cabin, two outrigger
pontoons ending in engines, a tall mast with a pale emblem sail, a long gun on the bow.
"""

import json
import math
import os
import struct
import sys
from itertools import pairwise

# material name -> (retail texture, fallback rgba, mapping, texture size in world units).
# mapping: "side" planks run along y and the frieze row sits on the gunwale (v from the top of the part); "tile" repeats by the face's
# dominant axis; "fit" stretches the whole texture over the part (sails, discs, flags).
MATS = {
    "hull": ("o_main_body", (0.45, 0.32, 0.2, 1), "side", 4.0),
    "pontoon": ("o_pontoon_body", (0.45, 0.32, 0.2, 1), "side", 4.0),
    "deck": ("deck_main", (0.6, 0.4, 0.28, 1), "tile", 3.0),
    "cabin": ("cutter porthole3", (0.5, 0.35, 0.22, 1), "tile", 3.0),
    "plate": ("d_pon_main", (0.5, 0.5, 0.5, 1), "tile", 3.0),
    "band": ("silver", (0.8, 0.8, 0.82, 1), "tile", 2.0),
    "wood": ("mast_mainwood", (0.4, 0.28, 0.16, 1), "tile", 2.0),
    "strut": ("sticks", (0.45, 0.32, 0.2, 1), "tile", 2.0),
    "rail": ("cruiser rail", (0.4, 0.4, 0.42, 1), "tile", 2.0),
    "engine": ("o_engine", (0.3, 0.4, 0.45, 1), "tile", 2.0),
    "glow": ("thruster_blue", (0.45, 0.8, 0.83, 1), "fit", 1.0),
    "hatch": ("o_gate2", (0.5, 0.5, 0.5, 1), "fit", 1.0),
    "sail": ("proc_front_tri_sail", (0.77, 0.8, 0.85, 1), "fit", 1.0),
    "flag": ("proc_flag2", (0.2, 0.45, 0.35, 1), "fit", 1.0),
    "gun": ("d_pon_main", (0.5, 0.5, 0.5, 1), "tile", 3.0),
}


def uv_of(p, n, mat, box):
    """Texture coordinates of point p (game axes) on a face of normal n, for material `mat`; box = (lo, hi) of the part."""
    _, _, mode, size = MATS[mat]
    lo, hi = box
    ax = max(range(3), key=lambda i: abs(n[i]))
    span = [max(hi[i] - lo[i], 1e-6) for i in range(3)]
    if mode == "fit":
        a, b = {0: (1, 2), 1: (0, 2), 2: (0, 1)}[ax]
        u, v = (p[a] - lo[a]) / span[a], 1 - (p[b] - lo[b]) / span[b]
        return u, v
    if mode == "side" and ax == 0:
        return p[1] / size, (hi[2] - p[2]) / span[2]
    if ax == 0:
        return p[1] / size, -p[2] / size
    if ax == 1:
        return p[0] / size, -p[2] / size
    return p[1] / size, p[0] / size


# (y, half width, z bottom, z top) of the central hull: a long slim planked hull, pointed bow, broad transom
HULL = [
    (-9, 1.0, 0.9, 2.4),
    (-6, 1.35, 0.8, 2.6),
    (-1, 1.5, 0.8, 2.7),
    (4, 1.2, 0.9, 2.6),
    (8, 0.6, 1.3, 2.4),
    (10.5, 0.12, 1.7, 2.2),
]
# an outrigger pontoon: long, slim, a flat transom for its engine
PONTOON = [
    (-8.5, 0.55, 1.1, 2.1),
    (-6, 0.8, 0.9, 2.3),
    (0, 0.85, 0.9, 2.4),
    (4, 0.7, 1.1, 2.3),
    (6.5, 0.15, 1.5, 2.0),
]
PX = 3.6  # pontoon centre line


def prof(st, y):
    for a, b in pairwise(st):
        if a[0] <= y <= b[0]:
            t = (y - a[0]) / (b[0] - a[0])
            return [a[i] + (b[i] - a[i]) * t for i in (1, 2, 3)]
    return list(st[-1][1:])


def ring(st, y, dx=0.0, k=1.0):
    w, zb, zt = prof(st, y)
    zc, h = (zb + zt) / 2, (zt - zb) / 2
    pts = [(0, zt), (w * 0.7, zc + h * 0.85), (w, zc), (w * 0.6, zb + h * 0.1), (0, zb)]
    full = pts + [(-x, z) for x, z in reversed(pts[1:-1])]
    return [(dx + x * k, y, zc + (z - zc) * k) for x, z in full]


def loft(st, y0, y1, dx=0.0, k=1.0, n=6):
    """A hull slice from y0 to y1 with end caps."""
    rs = [ring(st, y0 + (y1 - y0) * i / n, dx, k) for i in range(n + 1)]
    m = len(rs[0])
    v = [p for r in rs for p in r]
    f = [
        (i * m + j, i * m + (j + 1) % m, i * m + (j + 1) % m + m, i * m + j + m)
        for i in range(n)
        for j in range(m)
    ]
    return v, [*f, tuple(range(m)), tuple(range(n * m + m - 1, n * m - 1, -1))]


def prism(poly, place, thick):
    n = len(poly)
    v = [place(u, w, 0) for u, w in poly] + [place(u, w, thick) for u, w in poly]
    return v, [tuple(range(n)), tuple(range(2 * n - 1, n - 1, -1))] + [
        (i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)
    ]


def cyl(c, r, length, axis=1, sides=10):
    """Cylinder centred on c along axis (0 x, 1 y, 2 z)."""
    a, b = [i for i in range(3) if i != axis]
    v = []
    for end in (-length / 2, length / 2):
        for i in range(sides):
            t = 2 * math.pi * i / sides
            p = [0, 0, 0]
            p[axis], p[a], p[b] = end, r * math.cos(t), r * math.sin(t)
            v.append(tuple(c[j] + p[j] for j in range(3)))
    f = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
    return v, [*f, tuple(range(sides)), tuple(range(2 * sides - 1, sides - 1, -1))]


def box(c, s):
    v = [
        (c[0] + sx * s[0] / 2, c[1] + sy * s[1] / 2, c[2] + sz * s[2] / 2)
        for sz in (-1, 1)
        for sy in (-1, 1)
        for sx in (-1, 1)
    ]
    return v, [
        (0, 1, 3, 2),
        (4, 6, 7, 5),
        (0, 4, 5, 1),
        (2, 3, 7, 6),
        (0, 2, 6, 4),
        (1, 5, 7, 3),
    ]


def tris(part):
    """(verts, polys, material) -> [(a, c, b, normal, material)] in game axes, facing away from the part's centre.
    A material given as (side, top) is split by the face's normal: faces that look up take `top`."""
    v, polys, mat = part
    mid = [sum(p[i] for p in v) / len(v) for i in range(3)]
    out = []
    for poly in polys:
        for k in range(1, len(poly) - 1):
            a, b, c = (v[poly[0]], v[poly[k]], v[poly[k + 1]])
            u, w = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
            n = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ]
            if sum(n[i] * (a[i] + b[i] + c[i] - 3 * mid[i]) for i in range(3)) < 0:
                b, c, n = c, b, [-x for x in n]
            ln = math.sqrt(sum(x * x for x in n)) or 1
            n = [x / ln for x in n]
            m = (mat[1] if n[2] > 0.6 else mat[0]) if isinstance(mat, tuple) else mat
            out.append((a, c, b, n, m))  # winding (a, c, b) of game axes
    return out


def hardpoint(name, kind, pos, yaw=0.0):
    """An empty glTF node at a game-axes position, facing yaw about up (0 forward, positive to port)."""
    return {
        "name": name,
        "translation": [pos[0], pos[2], -pos[1]],
        "rotation": [0, math.sin(yaw / 2), 0, math.cos(yaw / 2)],
        "extras": {"kind": kind, "rot_raw": [yaw, 0, 0]},
    }


def glb(path, nodes, points=()):
    """nodes: {name: [(verts, polys, material), ...]}; one mesh + node each, one primitive per material.
    points: hardpoints the engine reads, (name, kind, (x, y, z) in game axes, yaw): empty nodes with an extras.kind (docs/modding.md)."""
    blob, views, accs, meshes = bytearray(), [], [], []
    names = list(MATS)
    mat_json = [
        {
            "name": "retail:" + MATS[n][0],
            "pbrMetallicRoughness": {
                "baseColorFactor": list(MATS[n][1]),
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            },
        }
        for n in names
    ]

    def add(data, typ, count, lo=None, hi=None):
        views.append(
            {
                "buffer": 0,
                "byteOffset": len(blob),
                "byteLength": len(data),
                "target": 34962,
            }
        )
        blob.extend(data)
        a = {
            "bufferView": len(views) - 1,
            "componentType": 5126,
            "count": count,
            "type": typ,
        }
        if lo:
            a["min"], a["max"] = lo, hi
        accs.append(a)
        return len(accs) - 1

    for name, parts in nodes.items():
        faces = [f for part in parts for f in _part_faces(part)]
        prims = []
        for m in names:
            pos, nrm, uvs = [], [], []
            for a, c, b, n, mm, box_ in faces:
                if mm != m:
                    continue
                for p in (a, c, b):
                    pos.append((p[0], p[2], -p[1]))
                    nrm.append((n[0], n[2], -n[1]))
                    uvs.append(uv_of(p, n, m, box_))
            if not pos:
                continue
            lo, hi = (
                [min(p[i] for p in pos) for i in range(3)],
                [max(p[i] for p in pos) for i in range(3)],
            )
            pa = add(
                b"".join(struct.pack("<3f", *p) for p in pos), "VEC3", len(pos), lo, hi
            )
            na = add(b"".join(struct.pack("<3f", *p) for p in nrm), "VEC3", len(nrm))
            ta = add(b"".join(struct.pack("<2f", *p) for p in uvs), "VEC2", len(uvs))
            prims.append(
                {
                    "attributes": {"POSITION": pa, "NORMAL": na, "TEXCOORD_0": ta},
                    "material": names.index(m),
                }
            )
        meshes.append({"name": name, "primitives": prims})
    doc = {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(meshes) + len(points)))}],
        "nodes": [{"name": m["name"], "mesh": i} for i, m in enumerate(meshes)]
        + [hardpoint(*p) for p in points],
        "meshes": meshes,
        "materials": mat_json,
        "accessors": accs,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = json.dumps(doc).encode()
    js += b" " * (-len(js) % 4)
    blob += b"\0" * (-len(blob) % 4)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(
            struct.pack("<4sII", b"glTF", 2, 28 + len(js) + len(blob))
            + struct.pack("<I4s", len(js), b"JSON")
            + js
            + struct.pack("<I4s", len(blob), b"BIN\0")
            + blob
        )


def _part_faces(part):
    """The part's faces with the part's bounding box (for the mappings that stretch a texture over the part)."""
    v = part[0]
    box_ = (
        [min(p[i] for p in v) for i in range(3)],
        [max(p[i] for p in v) for i in range(3)],
    )
    return [(*f, box_) for f in tris(part)]


# Hunter's mounts, engines, wake and boarding point. The engine reads these; ship.json only names the gun.
POINTS = [
    ("Forward_Heavymount", "GunPlacement", (0, 6.2, 2.62), 0),
    ("Engine_Port", "EnginePortPlacement", (-PX, -9.2, 1.6), math.pi),
    ("Engine_Center", "EnginePortPlacement", (0, -9.5, 1.6), math.pi),
    ("Engine_Starboard", "EnginePortPlacement", (PX, -9.2, 1.6), math.pi),
    ("Wake1", "WakePlacement", (0, -9.0, 0.9), math.pi),
    ("Boarding1", "BoardingEffectPoint", (0, -3.5, 2.8), 0),
]


def hull():
    """The ship's layers: each mesh node is a hit mesh and a damage section; "sail" or "mast" in a name makes it cloth.

    Hull-like layers use (side, top) materials: planked sides with the frieze on the gunwale, plank decking on top."""
    n = {}
    side = ("hull", "deck")
    n["Hunter nose"] = [
        (*loft(HULL, 3, 10.5), side),
        (*loft(HULL, 4.4, 4.9, k=1.04), "band"),
        (*loft(HULL, 7.2, 7.7, k=1.04), "band"),
        (*box((0, 6.2, 2.62), (1.5, 2.4, 0.26)), "plate"),  # the gun's raised platform
        (
            *cyl((0, 1.6, 2.75), 0.55, 0.12, axis=2, sides=14),
            "hatch",
        ),  # an emblem hatch on the foredeck
    ]
    n["Hunter midship"] = [
        (*loft(HULL, -3, 3), side),
        (*loft(HULL, -0.9, -0.4, k=1.04), "band"),
        (*loft(HULL, 1.3, 1.8, k=1.04), "band"),
    ]
    n["Hunter stern"] = [
        (*loft(HULL, -9, -3), side),
        (*loft(HULL, -7.6, -7.1, k=1.04), "band"),
        (*loft(HULL, -4.4, -3.9, k=1.04), "band"),
        (*box((0, -4.6, 3.55), (1.9, 3.4, 1.5)), "cabin"),  # raised wheelhouse
        (*box((0, -4.6, 4.35), (2.1, 3.6, 0.12)), "deck"),
        (*box((-1.12, -1.0, 3.0), (0.07, 6.0, 0.5)), "rail"),
        (*box((1.12, -1.0, 3.0), (0.07, 6.0, 0.5)), "rail"),
        (*cyl((0, -9.5, 1.6), 0.6, 0.9, sides=12), "engine"),
        (*cyl((0, -10.0, 1.6), 0.45, 0.1, sides=12), "glow"),
    ]
    for s in (-1, 1):
        name = "Hunter port" if s < 0 else "Hunter starboard"
        n[name + " pontoon"] = [
            (*loft(PONTOON, -8.5, 6.5, dx=s * PX), ("pontoon", "deck")),
            (*loft(PONTOON, -3.2, -2.7, dx=s * PX, k=1.05), "band"),
            (*loft(PONTOON, 1.8, 2.3, dx=s * PX, k=1.05), "band"),
            (*box((s * 2.3, -3.0, 2.0), (2.6, 0.45, 0.3)), "strut"),
            (*box((s * 2.3, 2.0, 2.0), (2.6, 0.45, 0.3)), "strut"),
            (*cyl((s * PX, -9.0, 1.6), 0.5, 0.8, sides=12), "engine"),
            (*cyl((s * PX, -9.45, 1.6), 0.38, 0.1, sides=12), "glow"),
        ]
    kite = [
        (-1.0, 3.4),
        (-6.0, 7.4),
        (-2.8, 13.4),
        (1.6, 8.0),
    ]  # (y, z): a diamond of cloth beside the mast
    n["Hunter mast and sail"] = [
        (*cyl((0, -1.5, 8.3), 0.16, 11.4, axis=2, sides=8), "wood"),
        (*cyl((0, -1.5, 1.0), 0.22, 0.2, axis=2, sides=8), "wood"),
        (*prism(kite, lambda u, w, d: (0.35 + d, u, w), 0.05), "sail"),
        (
            *prism(
                [(-2.8, 13.4), (-2.8, 12.7), (-5.6, 13.05)],
                lambda u, w, d: (d, u, w),
                0.04,
            ),
            "flag",
        ),
    ]
    return n


def gun():
    """Turret base ring and a long barrel along +y; muzzle at (0, 4.6, 0.5)."""
    base = [
        (*cyl((0, 0, 0.15), 0.5, 0.3, 2, 16), "gun"),
        (*cyl((0, 0, 0.45), 0.3, 0.9, 0), "gun"),
    ]
    barrel = [
        (*box((0, 0.2, 0.5), (0.42, 2.4, 0.36)), "gun"),
        (*cyl((0, 2.6, 0.5), 0.09, 4.0), "gun"),
        (*cyl((0, 2.0, 0.5), 0.14, 0.25), "gun"),
        (*cyl((0, 4.45, 0.5), 0.16, 0.35, sides=8), "gun"),
    ]
    return {"gun_Hunter_Layer1": base}, {"gun_Hunter_Layer2": barrel}


if __name__ == "__main__":
    out = sys.argv[1]
    glb(os.path.join(out, "ships/Ship_Hunter/model.glb"), hull(), POINTS)
    b1, b2 = gun()
    glb(os.path.join(out, "models/weapons/gun_Hunter_Layer1.glb"), b1)
    glb(os.path.join(out, "models/weapons/gun_Hunter_Layer2.glb"), b2)
