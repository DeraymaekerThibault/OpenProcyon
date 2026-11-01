"""Skinned animals (space dragon, whale, manta ray): bones, vertex weights and the scene's looping clip, for mdb_to_glb.

Layout (own words, read from the retail files; docs/re/effects.md "Animal skeletons"):
- MDB model record: after the faces, a count of influence maps, each a sized record holding a count and that many
  (u16 vertex index, f32 weight) pairs. Map k belongs to bone k of the bone table.
- MDB tail, right after the texture table: the bone table: a count, then per bone a sized record: name (length-prefixed)
  and a 4x4 row-vector matrix, the inverse of the bone's bind pose in model space.
- MSB scene: the bone hierarchy and rest pose (position, yaw/pitch/roll), and the clips. A node's rotation is
  Rz(yaw) Rx(pitch) Ry(roll) (fits the 32 dragon bind matrices); clip keys are offsets added to the rest position and to the
  rest Euler angles (as the original does).
PROVISIONAL: weights are normalised per vertex (stored values are small and do not sum to one), at most four kept.
"""

import itertools
import math
import struct
from pathlib import Path

import numpy as np

from tools.gamefs import ci_file
from tools.msb import parse_scene

C = np.array(
    [[1, 0, 0], [0, 0, 1], [0, -1, 0]], float
)  # game (x, y, z) -> glTF (x, z, -y)
FPS = 30


def _rot(e):
    def r(axis, a):
        c, s = math.cos(a), math.sin(a)
        m = np.eye(3)
        i, j = [(1, 2), (2, 0), (0, 1)][axis]
        m[i, i] = m[j, j] = c
        m[i, j], m[j, i] = -s, s
        return m

    return r(2, e[0]) @ r(0, e[1]) @ r(1, e[2])


def _quat(m):
    """Unit quaternion (x, y, z, w) of a rotation matrix."""
    w = math.sqrt(max(0.0, 1 + m[0, 0] + m[1, 1] + m[2, 2])) / 2
    x = math.copysign(
        math.sqrt(max(0.0, 1 + m[0, 0] - m[1, 1] - m[2, 2])) / 2, m[2, 1] - m[1, 2]
    )
    y = math.copysign(
        math.sqrt(max(0.0, 1 - m[0, 0] + m[1, 1] - m[2, 2])) / 2, m[0, 2] - m[2, 0]
    )
    z = math.copysign(
        math.sqrt(max(0.0, 1 - m[0, 0] - m[1, 1] + m[2, 2])) / 2, m[1, 0] - m[0, 1]
    )
    n = math.sqrt(x * x + y * y + z * z + w * w)
    return [x / n, y / n, z / n, w / n]


def _hermite(keys, t):
    """One channel at time t: keys are [time, value, smoothing, tension, continuity, bias, in tangent, out tangent]. The segment's
    first key picks the curve: 0 linear, 1 Hermite with the stored tangents, 2 Kochanek-Bartels from the values (the original's
    incoming tangent is only the chord: its look-ahead index is clamped wrongly, kept here). Values are held outside the keys."""
    if len(keys) == 1 or t <= keys[0][0]:
        return keys[0][1]
    for k, (a, b) in enumerate(itertools.pairwise(keys)):
        if t <= b[0]:
            u = (t - a[0]) / max(b[0] - a[0], 1e-6)
            if a[2] == 0:
                return a[1] + (b[1] - a[1]) * u
            h = (
                2 * u**3 - 3 * u**2 + 1,
                u**3 - 2 * u**2 + u,
                -2 * u**3 + 3 * u**2,
                u**3 - u**2,
            )
            if a[2] == 1:
                s0, s1 = a[7], b[6]
            else:
                p = keys[k - 1] if k > 0 else a
                t0, c0, b0 = a[3:6]
                t1, c1, b1 = b[3:6]
                s0 = (
                    (1 - t0) * (1 + c0) * (1 + b0) * (a[1] - p[1])
                    + (1 - t0) * (1 - c0) * (1 - b0) * (b[1] - a[1])
                ) * ((b[0] - a[0]) / max(b[0] - p[0], 1e-6))
                s1 = (1 - t1) * (1 - c1) * (1 + b1) * (b[1] - a[1])
            return h[0] * a[1] + h[1] * s0 + h[2] * b[1] + h[3] * s1
    return keys[-1][1]


def read(data, vcount):
    """(bone names, inverse bind matrices as column-vector 4x4, influence maps) of a skinned MDB, or None."""
    o = 16 + 4
    size = struct.unpack_from("<I", data, 16)[0]
    m = data[o : o + size]
    p = 8 + vcount * 36
    p += 4 + struct.unpack_from("<I", m, p)[0] * 12
    n = struct.unpack_from("<I", m, p)[0]
    p += 4
    maps = []
    for _ in range(n):
        s, c = struct.unpack_from("<2I", m, p)
        maps.append([struct.unpack_from("<Hf", m, p + 8 + 6 * k) for k in range(c)])
        p += 4 + s
    if not maps:
        return None
    # The bone table follows the texture table (records of their own size each).
    q = o + size
    for _ in range(struct.unpack_from("<I", data, q)[0]):
        q += 4 + struct.unpack_from("<I", data, q + 4)[0]
    q += 4
    count = struct.unpack_from("<I", data, q)[0]
    q += 4
    names, inv = [], []
    for _ in range(count):
        s = struct.unpack_from("<I", data, q)[0]
        rec = data[q + 4 : q + 4 + s]
        q += 4 + s
        ln = struct.unpack_from("<I", rec, 0)[0]
        names.append(rec[4 : 4 + ln].decode("latin-1"))
        inv.append(np.array(struct.unpack_from("<16f", rec, 4 + ln)).reshape(4, 4).T)
    if count != len(maps):
        raise ValueError(f"{count} bones but {len(maps)} influence maps")
    return names, inv, maps


def build(root, scene, data, vcount, add_view, accessors, nodes):
    """Adds the skeleton nodes, the vertex joint/weight accessors, the skin and the clip; returns
    (extra primitive attributes, skin, animations, root node index) or None for an unskinned mesh."""
    got = read(data, vcount)
    if not got:
        return None
    names, inv, maps = got
    sc = parse_scene(ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes())
    el = {e["id"]: e for e in sc["nodes"] + sc["meshes"] + sc["bones"]}
    bones = sorted(sc["bones"], key=lambda e: e["id"])
    ids = [b["id"] for b in bones]

    def local(e, d=(0,) * 6):
        m = np.eye(4)
        m[:3, :3] = _rot([e["rot"][k] + d[3 + k] for k in range(3)])
        m[:3, 3] = [e["pos"][k] + d[k] for k in range(3)]
        return m

    def chain(e):  # world transform of a non-bone ancestor chain
        return np.eye(4) if e is None else chain(el.get(e["parent"])) @ local(e)

    def trs(m):
        return [*(C @ m[:3, 3])], _quat(C @ m[:3, :3] @ C.T)

    first = len(nodes)
    rest = {}
    for b in bones:
        m = local(b)
        if b["parent"] not in ids:
            m = chain(el.get(b["parent"])) @ m
        rest[b["id"]] = m
        t, r = trs(m)
        nodes.append({"name": b["name"], "translation": t, "rotation": r})
    for i, b in enumerate(bones):
        kids = [first + k for k, c in enumerate(bones) if c["parent"] == b["id"]]
        if kids:
            nodes[first + i]["children"] = kids
    roots = [first + i for i, b in enumerate(bones) if b["parent"] not in ids]
    nodes.append({"name": "skeleton", "children": roots})

    def world(b):
        m = rest[b["id"]]
        return m if b["parent"] not in ids else world(el[b["parent"]]) @ m

    by_name = {b["name"]: i for i, b in enumerate(bones)}
    ibm = []
    for b in bones:
        g = (
            inv[names.index(b["name"])]
            if b["name"] in names
            else np.linalg.inv(world(b))
        )
        cg = np.eye(4)
        cg[:3, :3] = C
        ibm.append((cg @ g @ cg.T).T.flatten())  # glTF: column-major
    accessors.append(
        {
            "bufferView": add_view(
                struct.pack(f"<{16 * len(ibm)}f", *np.concatenate(ibm))
            ),
            "componentType": 5126,
            "count": len(ibm),
            "type": "MAT4",
        }
    )
    skin = {
        "joints": list(range(first, first + len(bones))),
        "inverseBindMatrices": len(accessors) - 1,
        "skeleton": len(nodes) - 1,
    }

    per = [[] for _ in range(vcount)]
    for k, entries in enumerate(maps):
        for v, w in entries:
            if (
                names[k] in by_name
            ):  # the whale's table also lists a 'NULL' bone the scene does not have
                per[v].append((w, by_name[names[k]]))
    joints, weights = bytearray(), bytearray()
    for infl in per:
        infl = sorted(infl, reverse=True)[:4] or [(1.0, 0)]
        total = sum(w for w, _ in infl)
        infl += [(0.0, 0)] * (4 - len(infl))
        joints += struct.pack("<4H", *(j for _, j in infl))
        weights += struct.pack("<4f", *(w / total for w, _ in infl))
    accessors.append(
        {
            "bufferView": add_view(bytes(joints), 34962),
            "componentType": 5123,
            "count": vcount,
            "type": "VEC4",
        }
    )
    accessors.append(
        {
            "bufferView": add_view(bytes(weights), 34962),
            "componentType": 5126,
            "count": vcount,
            "type": "VEC4",
        }
    )
    attrs = {"JOINTS_0": len(accessors) - 2, "WEIGHTS_0": len(accessors) - 1}

    animations = []
    for clip in sc["animations"][:1]:
        frames = max(2, round(clip["duration"] * FPS) + 1)
        times = [clip["duration"] * f / (frames - 1) for f in range(frames)]
        accessors.append(
            {
                "bufferView": add_view(struct.pack(f"<{frames}f", *times)),
                "componentType": 5126,
                "count": frames,
                "type": "SCALAR",
                "min": [0.0],
                "max": [clip["duration"]],
            }
        )
        ta = len(accessors) - 1
        samplers, channels = [], []
        for mo in clip["motions"]:
            b = el.get(mo["node"])
            if b is None or b["id"] not in ids:
                continue
            ts, rs = [], []
            for t in times:
                m = local(b, [_hermite(ch, t) for ch in mo["channels"]])
                if b["parent"] not in ids:
                    m = chain(el.get(b["parent"])) @ m
                tt, rr = trs(m)
                ts += tt
                rs += rr
            for path, vals, kind in (
                ("translation", ts, "VEC3"),
                ("rotation", rs, "VEC4"),
            ):
                accessors.append(
                    {
                        "bufferView": add_view(struct.pack(f"<{len(vals)}f", *vals)),
                        "componentType": 5126,
                        "count": frames,
                        "type": kind,
                    }
                )
                samplers.append(
                    {
                        "input": ta,
                        "output": len(accessors) - 1,
                        "interpolation": "LINEAR",
                    }
                )
                channels.append(
                    {
                        "sampler": len(samplers) - 1,
                        "target": {"node": first + ids.index(b["id"]), "path": path},
                    }
                )
        animations.append(
            {"name": clip["name"], "samplers": samplers, "channels": channels}
        )
    return attrs, skin, animations, len(nodes) - 1
