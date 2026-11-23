"""Reader for BD_Meshes/*.mdb (layout per public TPForm source; unknown fields are kept raw)."""

import struct
from dataclasses import dataclass, field

from tools import importlog


class MdbError(ValueError):
    pass


@dataclass
class Vertex:
    pos: tuple
    uv: tuple
    unk: tuple  # packed vertex normal (azimuth, elevation) radians, see normal()
    color: int  # 0xFFFFFFFF in every file seen so far


def normal(v):
    """Outward unit normal, game frame. Fitted against face normals of retail meshes (mean dot ~0.93)."""
    import math

    az, el = v.unk
    return (-math.cos(el) * math.sin(az), math.cos(el) * math.cos(az), math.sin(el))


@dataclass
class Model:
    id: int
    vertices: list
    faces: list  # (v1, v2, v3, material_index)
    anim_frames: int = 0


@dataclass
class Mdb:
    header: tuple
    models: list
    textures: list  # texture names exactly as stored (.tga)
    trailing: bytes = field(repr=False, default=b"")


class _R:
    def __init__(self, data):
        self.d, self.o = data, 0

    def take(self, n):
        if n < 0 or self.o + n > len(self.d):
            raise MdbError(
                f"read of {n} bytes at {self.o} runs past end ({len(self.d)})"
            )
        b = self.d[self.o : self.o + n]
        self.o += n
        return b

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]


def _sized(r, minimum):
    n = r.u32()
    if n < minimum:
        raise MdbError(f"record at {r.o - 4} has size {n}, expected >= {minimum}")
    return r.take(n)


def parse(data):
    r = _R(data)
    header = struct.unpack("<4I", r.take(16))
    models = []
    for _ in range(header[3]):
        m = _R(_sized(r, 12))
        mid, vcount = m.u32(), m.u32()
        verts = []
        for _ in range(vcount):
            v = struct.unpack_from("<8f", p := _sized(m, 32), 0)
            verts.append(
                Vertex(v[0:3], v[3:5], v[5:7], struct.unpack_from("<I", p, 28)[0])
            )
        faces = []
        for _ in range(m.u32()):
            f = struct.unpack_from("<4H", _sized(m, 8), 0)
            if max(f[:3]) >= vcount:
                raise MdbError(
                    f"model {mid}: face index {max(f[:3])} >= {vcount} vertices"
                )
            faces.append(f)
        frames = m.u32()
        models.append(Model(mid, verts, faces, frames))
    textures = []
    for _ in range(r.u32()):
        p = _sized(r, 76)
        n = struct.unpack_from("<I", p, 0)[0]
        textures.append(p[4 : 4 + n].split(b"\0")[0].decode("latin-1"))
    return Mdb(header, models, textures, data[r.o :])


# Tag names of the record stream. Each file ends with its own table (tag number = position in the list), and the numbers shift
# between files (a mesh whose root is a leaf has no child tags), so the reader looks them up; this is the table of the islands.
_TREE_TAGS = [
    "MeshData", "Root", "LocalBasis", "Position", "LookAt Vector Length", "Orientation - Cross", "Orientation - Forward",
    "Orientation - Up", "Length", "Radius", "Level", "HasLeftChild", "HasRightChild", "LeftChild",
    "Valid Collision Triangle Indices - Size", "Valid Collision Triangle Indices - Element", "RightChild", "MaxLevel",
]  # fmt: skip


def _tag_table(trailing):
    """{tag name: number} from the table at the end of the trailing block, or the islands' table when there is none."""
    i = trailing.find(b"MeshData")
    if i >= 8 and struct.unpack_from("<I", trailing, i - 4)[0] == 8:
        r = _R(trailing)
        r.o = i - 8
        try:
            names = [r.take(r.u32()).decode("latin-1") for _ in range(r.u32())]
            return {n: k for k, n in enumerate(names)}
        except (MdbError, struct.error) as e:
            importlog.degraded(
                "MDB collision tag table", f"{e}; using the islands' table"
            )
    return {n: k for k, n in enumerate(_TREE_TAGS)}


def _box_node(b, tags):
    """One node body of the box tree: a stream of `[tag u32][value]` fields (see `box_tree`)."""
    t = lambda name: tags.get(name, -1)  # noqa: E731
    r = _R(b)
    node = {"children": [], "tris": []}
    while r.o < len(b):
        tag = r.u32()
        if tag == t("LocalBasis"):  # centre, an unused value, the three axes
            basis = _R(_sized(r, 0))
            axes = {}
            while basis.o < len(basis.d):
                k = basis.u32()
                if k == t("Position"):
                    node["centre"] = struct.unpack("<3f", basis.take(12))
                elif k == t("LookAt Vector Length"):
                    basis.take(4)
                elif k in (
                    t("Orientation - Cross"),
                    t("Orientation - Forward"),
                    t("Orientation - Up"),
                ):
                    axes[k] = struct.unpack("<3f", basis.take(12))
                else:
                    raise MdbError(f"box tree: unknown basis tag {k}")
            try:
                node["axes"] = [
                    axes[t(n)]
                    for n in (
                        "Orientation - Cross",
                        "Orientation - Forward",
                        "Orientation - Up",
                    )
                ]
            except KeyError:
                raise MdbError("box tree: basis without all three axes") from None
        elif tag == t("Length"):
            node["half"] = struct.unpack("<3f", r.take(12))
        elif tag == t("Radius"):
            node["radius"] = struct.unpack("<f", r.take(4))[0]
        elif tag == t("Level"):
            node["level"] = r.u32()
        elif tag in (t("HasLeftChild"), t("HasRightChild")):
            r.take(1)  # the child blocks themselves say whether a child exists
        elif tag in (t("LeftChild"), t("RightChild")):
            node["children"].append(_box_node(_sized(r, 0), tags))
        elif tag == t("Valid Collision Triangle Indices - Size"):
            for _ in range(r.u32()):
                if r.u32() != t("Valid Collision Triangle Indices - Element"):
                    raise MdbError("box tree: expected a triangle index element")
                node["tris"].append(r.u32())
        else:
            raise MdbError(f"box tree: unknown tag {tag}")
    for key in ("centre", "axes", "half", "level"):
        if key not in node:
            raise MdbError(f"box tree: node without {key}")
    return node


def box_tree(trailing):
    """The tree of oriented boxes after the textures (`Mdb.trailing`), or None when there is none.

    Header (bounding box, centre, radius, size), then the root as a block `[1][size][body]`; found as the first place where tag 1 is
    followed, after the size, by tag 2. A node body is a stream of tagged fields: LocalBasis (a block: Position = box centre,
    LookAt Vector Length unused, Orientation Cross/Forward/Up = the three unit axes), Length (half extents along those axes), Radius,
    Level (root 0), HasLeftChild / HasRightChild (one byte each), LeftChild / RightChild (blocks holding a node body) and the node's
    valid triangles (a count, then an element tag and an index each). The islands' tag numbers are 2 LocalBasis, 3, 4, 5, 6, 7, 8
    Length, 9 Radius, 10 Level, 11, 12, 13 LeftChild, 14 count, 15 element, 16 RightChild; other files number them from their own
    table at the end of the block, which is what is read. Returns nested dicts {centre, axes, half, radius, level, children, tris}
    in the mesh frame."""
    tags = _tag_table(trailing)
    basis = tags.get("LocalBasis", 2)
    for o in range(0, len(trailing) - 12, 4):
        tag, size, nxt = struct.unpack_from("<3I", trailing, o)
        if (
            tag == tags.get("Root", 1)
            and nxt == basis
            and o + 8 + size <= len(trailing)
        ):
            return _box_node(trailing[o + 8 : o + 8 + size], tags)
    return None
