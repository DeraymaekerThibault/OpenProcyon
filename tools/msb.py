"""Reader for BD_MeshScenes/*.msb node records (hardpoints, damage sections, ...).

The file is a tagged stream with nested/variant blocks that are not fully understood, so this scans for
the node record pattern instead of walking a grammar:
  02 id | 05 parent | 06 ? | 01 name | 07 3*u32 | 08 f32 *6 | 09 u32 | [0a u32 | 0b str | 0c str]
Each tag is a u32; `str` is u32 length + bytes. The six floats are x, y, z, yaw, ?, ? (radians; only
yaw is non-zero in the Navy Cutter). Positions are in the parent mesh's local frame, which for the
cutter equals the MDB vertex frame (mounts land inside their parent hull layer's bounding box).
"""

import struct
from dataclasses import dataclass


@dataclass
class Node:
    id: int
    parent: int
    name: str
    pos: tuple
    rot: tuple  # (yaw, ?, ?)
    kind: str = ""  # e.g. GunPlacement, DamageSection, FlagAttachmentPoint
    label: str = ""  # e.g. Port_Medmount1, bow_wood


def _u32(d, o):
    return struct.unpack_from("<I", d, o)[0] if o + 4 <= len(d) else None


def _str(d, o):
    n = _u32(d, o)
    if n is None or n > 256 or o + 4 + n > len(d):
        return None, o
    return d[o + 4 : o + 4 + n].decode("latin-1"), o + 4 + n


def parse(d):
    nodes, o = [], 0
    while o + 28 <= len(d):
        if not (
            _u32(d, o) == 2
            and _u32(d, o + 8) == 5
            and _u32(d, o + 16) == 6
            and _u32(d, o + 24) == 1
        ):
            o += 1
            continue
        nid, parent = _u32(d, o + 4), struct.unpack_from("<i", d, o + 12)[0]
        name, p = _str(d, o + 28)
        if name is None or _u32(d, p) != 7:
            o += 1
            continue
        p += 16
        fl = []
        for _ in range(6):
            if _u32(d, p) != 8:
                break
            fl.append(struct.unpack_from("<f", d, p + 4)[0])
            p += 8
        if len(fl) != 6 or _u32(d, p) != 9:
            o += 1
            continue
        p += 8
        kind = label = ""
        if _u32(d, p) == 10 and _u32(d, p + 8) == 11:
            k, q = _str(d, p + 12)
            # In some vanilla scenes (AsteroidXlarge, IslandDragon*, BaseCresentiaLoRez) this block nests the child node that
            # names the layer mesh instead of a text kind; leave it unread so the scan finds that child.
            if k is not None and k.isprintable() and _u32(d, q) == 12:
                node_label, q = _str(d, q + 4)
                if node_label is not None:
                    kind, label, p = k, node_label, q
        nodes.append(Node(nid, parent, name, tuple(fl[:3]), tuple(fl[3:]), kind, label))
        o = p
    return nodes
