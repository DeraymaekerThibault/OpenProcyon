"""Reader for BD_Meshes/*.mdb (layout per public TPForm source; unknown fields are kept raw)."""

import struct
from dataclasses import dataclass, field


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
