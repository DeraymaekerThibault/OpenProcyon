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


# ---- strict reader ---------------------------------------------------------------------------------------------------------
# Every scene ends with a table of the tag names it uses (count, then length-prefixed strings; it starts where the file's first
# u32 points). The tags in the stream are indices into that table, so their numbers differ between files. Reading by name parses
# every retail scene to its end, which the heuristic `parse` above does not promise (it misses some elements and keeps only the
# first attribute of a node).


class _Reader:
    def __init__(self, data):
        self.d, self.o = data, 0

    def u32(self):
        (v,) = struct.unpack_from("<I", self.d, self.o)
        self.o += 4
        return v

    def i32(self):
        (v,) = struct.unpack_from("<i", self.d, self.o)
        self.o += 4
        return v

    def f32(self):
        (v,) = struct.unpack_from("<f", self.d, self.o)
        self.o += 4
        return v

    def string(self):
        n = self.u32()
        s = self.d[self.o : self.o + n].decode("latin-1")
        self.o += n
        return s


def tag_table(data):
    r = _Reader(data)
    r.o = r.u32()
    names = [r.string() for _ in range(r.u32())]
    if r.o != len(data):
        raise ValueError("MSB tag table does not end the file")
    return names


def parse_scene(data):
    """The whole scene: {name, root, nodes, meshes, bones, animations}.
    Elements (nodes, meshes, bones): {id, parent (-1 none), type (0 node, 1 mesh layer, 2 bone), name, pivot, pos (x y z), rot (yaw
    pitch roll), attributes [(AttributeName, DescriptorName)]}; bones add `influence` and `rest_length`.
    Animations: {name, duration seconds, motions [{node id, channels}]}; six channels per motion (x, y, z position, yaw, pitch, roll
    in radians), each a list of keyframes (time, value, smoothing, tension, continuity, bias, in tangent, out tangent)."""
    names = tag_table(data)
    r = _Reader(data)

    def tag(name):
        got = r.u32()
        if got >= len(names) or names[got] != name:
            raise ValueError(
                f"MSB: expected tag {name!r} at {r.o - 4}, found {names[got] if got < len(names) else got}"
            )

    def element(bone=False):
        el = {}
        tag("ID")
        el["id"] = r.i32()
        tag("Parent ID")
        el["parent"] = r.i32()
        tag("Type")
        el["type"] = r.u32()
        tag("Name")
        el["name"] = r.string()
        tag("Pivot Position")
        el["pivot"] = [r.f32() for _ in range(3)]
        values = []
        for _ in range(6):
            tag("Element")
            values.append(r.f32())
        el["pos"], el["rot"] = values[:3], values[3:]
        tag("Attributes - Size")
        el["attributes"] = []
        for _ in range(r.u32()):
            tag("Attributes - Element")
            r.u32()
            tag("AttributeName")
            kind = r.string()
            tag("DescriptorName")
            el["attributes"].append((kind, r.string()))
        if bone:
            tag("Influence Map Name")
            el["influence"] = r.string()
            tag("Rest Length")
            el["rest_length"] = r.f32()
        return el

    def group(label, bone=False):
        tag(f"{label} - Size")
        out = []
        for _ in range(r.u32()):
            tag(f"{label} - Element")
            r.u32()
            out.append(element(bone))
        return out

    r.o = 12  # three header words: end of the data, 0, end of the data - 12
    tag("Name")
    scene = {"name": r.string()}
    tag("ID")
    scene["root"] = r.i32()
    scene["nodes"] = group("Nodes")
    scene["meshes"] = group("Meshes")
    scene["bones"] = group("Bones", bone=True)
    tag("Animations - Size")
    scene["animations"] = []
    for _ in range(r.u32()):
        tag("Animations - Element")
        r.u32()
        tag("Name")
        clip = {"name": r.string()}
        tag("Duration")
        clip["duration"] = r.f32()
        tag("Node Motion Count")
        clip["motions"] = []
        for _ in range(r.u32()):
            tag("Node ID")
            node = r.i32()
            tag("Motion")
            r.u32()
            channels = []
            for _ in range(6):
                tag("Channel")
                r.u32()
                tag("Keyframes - Size")
                keys = []
                for _ in range(r.u32()):
                    tag("Keyframes - Element")
                    r.u32()
                    tag("Time")
                    t = r.f32()
                    tag("Value")
                    v = r.f32()
                    tag("Smoothing")
                    smoothing = r.i32()
                    rest = []
                    for name in (
                        "Tension",
                        "Continuity",
                        "Bias",
                        "Incoming Tangent",
                        "Outgoing Tangent",
                    ):
                        tag(name)
                        rest.append(r.f32())
                    keys.append([t, v, smoothing, *rest])
                channels.append(keys)
            clip["motions"].append({"node": node, "channels": channels})
        scene["animations"].append(clip)
    if r.o != struct.unpack_from("<I", data, 0)[0]:
        raise ValueError("MSB data does not end where the header says")
    return scene
