"""Builds the meshes of mods/hunter (hull + sniper turret) as GLB, pure Python, no Blender.

    python3 tools/make_hunter.py mods/hunter

Own art, CC0. Game axes (x starboard, y forward, z up); GLB uses (x, z, -y) with winding (a, c, b).
"""
import json
import math
import os
import struct
import sys

# materials: name -> (rgba, metallic, roughness). "sail" in a name: the engine's sail shader/transparency.
MATS = {
    "Hunter hull": ((0.05, 0.055, 0.065, 1), 0.6, 0.4),
    "Hunter plate": ((0.16, 0.17, 0.19, 1), 0.8, 0.35),
    "Hunter accent": ((0.9, 0.35, 0.02, 1), 0.3, 0.5),
    "Hunter glass": ((0.1, 0.5, 0.7, 1), 0.1, 0.1),
    "Hunter sail": ((0.9, 0.4, 0.05, 0.75), 0.0, 0.8),
}
# (y, half width, z bottom, z top): a needle with a wide shoulder
ST = [(-6.5, 0.6, 1.6, 2.7), (-4, 1.1, 1.3, 3.2), (-1, 1.5, 1.2, 3.5), (2, 1.3, 1.4, 3.5), (5, 0.7, 1.9, 3.1), (8.5, 0.12, 2.3, 2.7)]


def prof(y):
    for a, b in zip(ST, ST[1:]):
        if a[0] <= y <= b[0]:
            t = (y - a[0]) / (b[0] - a[0])
            return [a[i] + (b[i] - a[i]) * t for i in (1, 2, 3)]
    return list(ST[-1][1:])


def ring(y, half):
    w, zb, zt = prof(y)
    zc, h = (zb + zt) / 2, (zt - zb) / 2
    pts = [(0, zt), (w * 0.7, zc + h * 0.85), (w, zc), (w * 0.6, zb + h * 0.1), (0, zb)]
    full = pts + [(-x, z) for x, z in reversed(pts[1:-1])]
    return [(x, y, z) for x, z in (pts if half else full)]


def loft(y0, y1, half=0, n=6):
    """A hull slice; half = +1 / -1 keeps only the starboard / port side."""
    rs = [[((half or 1) * x, y, z) for x, y, z in ring(y0 + (y1 - y0) * i / n, half)] for i in range(n + 1)]
    k = len(rs[0])
    v = [p for r in rs for p in r]
    f = [(i * k + j, i * k + (j + 1) % k, i * k + (j + 1) % k + k, i * k + j + k) for i in range(n) for j in range(k - 1 if half else k)]
    return v, f + [tuple(range(k)), tuple(range(n * k + k - 1, n * k - 1, -1))]


def prism(poly, place, thick):
    n = len(poly)
    v = [place(u, w, 0) for u, w in poly] + [place(u, w, thick) for u, w in poly]
    return v, [tuple(range(n)), tuple(range(2 * n - 1, n - 1, -1))] + [(i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)]


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
    return v, f + [tuple(range(sides)), tuple(range(2 * sides - 1, sides - 1, -1))]


def box(c, s):
    v = [(c[0] + sx * s[0] / 2, c[1] + sy * s[1] / 2, c[2] + sz * s[2] / 2) for sz in (-1, 1) for sy in (-1, 1) for sx in (-1, 1)]
    return v, [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]


def ellipsoid(c, r, rings=6, seg=10):
    v = [(c[0], c[1], c[2] - r[2]), (c[0], c[1], c[2] + r[2])]
    for i in range(1, rings):
        p = math.pi * i / rings
        for j in range(seg):
            t = 2 * math.pi * j / seg
            v.append((c[0] + r[0] * math.sin(p) * math.cos(t), c[1] + r[1] * math.sin(p) * math.sin(t), c[2] - r[2] * math.cos(p)))
    f = [(0, 2 + (j + 1) % seg, 2 + j) for j in range(seg)]
    f += [(1, 2 + (rings - 2) * seg + j, 2 + (rings - 2) * seg + (j + 1) % seg) for j in range(seg)]
    f += [(2 + i * seg + j, 2 + i * seg + (j + 1) % seg, 2 + (i + 1) * seg + (j + 1) % seg, 2 + (i + 1) * seg + j) for i in range(rings - 2) for j in range(seg)]
    return v, f


def tris(part):
    """(verts, polys, material) -> flat-shaded (positions, normals) in glTF axes, facing away from the part's centre."""
    v, polys, _ = part
    mid = [sum(p[i] for p in v) / len(v) for i in range(3)]
    pos, nrm = [], []
    for poly in polys:
        for k in range(1, len(poly) - 1):
            a, b, c = (v[poly[0]], v[poly[k]], v[poly[k + 1]])
            u, w = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
            n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
            if sum(n[i] * (a[i] + b[i] + c[i] - 3 * mid[i]) for i in range(3)) < 0:
                b, c, n = c, b, [-x for x in n]
            ln = math.sqrt(sum(x * x for x in n)) or 1
            for p in (a, c, b):  # winding (a, c, b) of game axes
                pos.append((p[0], p[2], -p[1]))
                nrm.append((n[0] / ln, n[2] / ln, -n[1] / ln))
    return pos, nrm


def glb(path, nodes):
    """nodes: {name: [(verts, polys, material name), ...]}; one mesh + node each, one primitive per material."""
    blob, views, accs, meshes, mats = bytearray(), [], [], [], {}
    for name in MATS:
        rgba, met, rough = MATS[name]
        mats[name] = len(mats)
    mat_json = [{"name": n, "pbrMetallicRoughness": {"baseColorFactor": list(MATS[n][0]), "metallicFactor": MATS[n][1], "roughnessFactor": MATS[n][2]}, **({"alphaMode": "BLEND"} if MATS[n][0][3] < 1 else {})} for n in MATS]

    def add(data, target, typ, count, lo=None, hi=None):
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        a = {"bufferView": len(views) - 1, "componentType": 5126, "count": count, "type": typ}
        if lo:
            a["min"], a["max"] = lo, hi
        accs.append(a)
        return len(accs) - 1

    for name, parts in nodes.items():
        prims = []
        for m in MATS:
            pos, nrm = [], []
            for part in (p for p in parts if p[2] == m):
                p, n = tris(part)
                pos += p
                nrm += n
            if not pos:
                continue
            lo, hi = [min(p[i] for p in pos) for i in range(3)], [max(p[i] for p in pos) for i in range(3)]
            pa = add(b"".join(struct.pack("<3f", *p) for p in pos), 34962, "VEC3", len(pos), lo, hi)
            na = add(b"".join(struct.pack("<3f", *p) for p in nrm), 34962, "VEC3", len(nrm))
            prims.append({"attributes": {"POSITION": pa, "NORMAL": na}, "material": mats[m]})
        meshes.append({"name": name, "primitives": prims})
    doc = {"asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": list(range(len(meshes)))}],
           "nodes": [{"name": m["name"], "mesh": i} for i, m in enumerate(meshes)], "meshes": meshes, "materials": mat_json,
           "accessors": accs, "bufferViews": views, "buffers": [{"byteLength": len(blob)}]}
    js = json.dumps(doc).encode()
    js += b" " * (-len(js) % 4)
    blob += b"\0" * (-len(blob) % 4)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, 28 + len(js) + len(blob)) + struct.pack("<I4s", len(js), b"JSON") + js + struct.pack("<I4s", len(blob), b"BIN\0") + blob)


def hull():
    L = "NavyCutterShip_Layer"  # the cutter's layer names, so ship.json sections and hit meshes carry over
    n = {}
    n[L + "1"] = [(*loft(3.5, 8.5), "Hunter hull"), (*ellipsoid((0, 3.6, 3.45), (0.55, 1.4, 0.4)), "Hunter glass"), (*cyl((0, 7.2, 2.95), 0.22, 2.2), "Hunter accent")]
    n[L + "2"] = [(*loft(-6.5, -3), "Hunter hull"), (*cyl((0, -6.6, 2.0), 0.75, 1.6, sides=12), "Hunter plate"), (*cyl((0, -7.45, 2.0), 0.5, 0.2, sides=12), "Hunter accent")]
    for s, i in ((-1, 3), (1, 4)):
        wing = [(s * x, y) for x, y in [(1.2, -2.2), (5.2, 1.6), (5.2, 2.8), (1.2, 2.4)]]  # swept forward
        tip = [(s * x, y) for x, y in [(5.2, 1.4), (5.2, 3.0), (5.6, 3.6), (5.6, 0.9)]]
        n[L + str(i)] = [(*loft(-3, 3.5, s), "Hunter hull"), (*prism(wing, lambda u, w, d: (u, w, 2.1 - d), 0.18), "Hunter plate"),
                         (*prism(tip, lambda u, w, d: (u, w, 2.1 - d), 0.18), "Hunter accent"), (*box((s * 0.9, 0.5, 3.55), (0.16, 6.4, 0.08)), "Hunter accent")]
    fin = [(-6.4, 2.6), (-3.2, 2.9), (-3.8, 5.6), (-5.4, 5.4)]
    for s, i in ((-1, 5), (1, 6)):
        n[L + str(i)] = [(*prism(fin, lambda u, w, d: (s * (0.9 + d), u, w), 0.06), "Hunter sail")]
    return n


def gun():
    """Turret base ring and a long barrel along +y; muzzle at (0, 4.6, 0.5)."""
    base = [(*cyl((0, 0, 0.15), 0.5, 0.3, 2, 16), "Hunter plate"), (*cyl((0, 0, 0.45), 0.3, 0.9, 0), "Hunter hull")]
    barrel = [(*box((0, 0.2, 0.5), (0.42, 2.4, 0.36)), "Hunter plate"), (*cyl((0, 2.6, 0.5), 0.09, 4.0), "Hunter hull"),
              (*cyl((0, 2.0, 0.5), 0.14, 0.25), "Hunter accent"), (*cyl((0, 4.45, 0.5), 0.16, 0.35, sides=8), "Hunter plate")]
    return {"gun_Hunter_Layer1": base}, {"gun_Hunter_Layer2": barrel}


if __name__ == "__main__":
    out = sys.argv[1]
    glb(os.path.join(out, "ships/Ship_Hunter/model.glb"), hull())
    b1, b2 = gun()
    glb(os.path.join(out, "models/weapons/gun_Hunter_Layer1.glb"), b1)
    glb(os.path.join(out, "models/weapons/gun_Hunter_Layer2.glb"), b2)
