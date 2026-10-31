import struct
import unittest

from tools.msb import parse, parse_scene

NAMES = [
    "Name", "ID", "Nodes - Size", "Nodes - Element", "Parent ID", "Type", "Pivot Position", "Element",
    "Attributes - Size", "Attributes - Element", "AttributeName", "DescriptorName", "Meshes - Size", "Bones - Size",
    "Animations - Size", "Animations - Element", "Duration", "Node Motion Count", "Node ID", "Motion", "Channel",
    "Keyframes - Size", "Keyframes - Element", "Time", "Value", "Smoothing", "Tension", "Continuity", "Bias",
    "Incoming Tangent", "Outgoing Tangent",
]  # fmt: skip


def scene():
    """A scene in the retail layout: one node with two attributes and one clip of one motion."""
    t = {n: i for i, n in enumerate(NAMES)}
    out = bytearray()

    def u(*v):
        out.extend(struct.pack(f"<{len(v)}I", *v))

    def s(text):
        u(len(text))
        out.extend(text.encode())

    u(0, 0, 0)  # the header words are patched below
    u(t["Name"])
    s("Test")
    u(t["ID"], 0)
    u(t["Nodes - Size"], 1)
    u(t["Nodes - Element"], 0)
    u(t["ID"], 0, t["Parent ID"], 0xFFFFFFFF, t["Type"], 0, t["Name"])
    s("Smoke1")
    u(t["Pivot Position"])
    out.extend(struct.pack("<3f", 0, 0, 0))
    for v in (1.0, 2.0, 3.0, 0.5, 0.0, 0.0):
        u(t["Element"])
        out.extend(struct.pack("<f", v))
    u(t["Attributes - Size"], 2)
    for kind, desc in (("PlayEffect", "default"), ("VolcanoSmoke", "Smoke1")):
        u(t["Attributes - Element"], 0, t["AttributeName"])
        s(kind)
        u(t["DescriptorName"])
        s(desc)
    u(t["Meshes - Size"], 0, t["Bones - Size"], 0)
    u(t["Animations - Size"], 1, t["Animations - Element"], 0, t["Name"])
    s("ANIMATION00")
    u(t["Duration"])
    out.extend(struct.pack("<f", 2.0))
    u(t["Node Motion Count"], 1, t["Node ID"], 0, t["Motion"], 0)
    for channel in range(6):
        u(t["Channel"], 0, t["Keyframes - Size"], 2 if channel == 0 else 0)
        for time, value in ((0.0, 0.0), (2.0, 5.0)) if channel == 0 else ():
            u(t["Keyframes - Element"], 0, t["Time"])
            out.extend(struct.pack("<f", time))
            u(t["Value"])
            out.extend(struct.pack("<f", value))
            u(t["Smoothing"], 2)
            for name, v in (
                ("Tension", 0),
                ("Continuity", 0),
                ("Bias", 0),
                ("Incoming Tangent", 0),
                ("Outgoing Tangent", value),
            ):
                u(t[name])
                out.extend(struct.pack("<f", v))
    table = len(out)
    u(len(NAMES))
    for n in NAMES:
        s(n)
    struct.pack_into("<III", out, 0, table, 0, table - 12)
    return bytes(out)


class StrictScene(unittest.TestCase):
    def test_nodes_with_every_attribute_and_animation_clips(self):
        sc = parse_scene(scene())
        (node,) = sc["nodes"]
        self.assertEqual(
            (node["name"], node["parent"], node["pos"]), ("Smoke1", -1, [1.0, 2.0, 3.0])
        )
        self.assertEqual(
            node["attributes"], [("PlayEffect", "default"), ("VolcanoSmoke", "Smoke1")]
        )
        (clip,) = sc["animations"]
        self.assertEqual((clip["name"], clip["duration"]), ("ANIMATION00", 2.0))
        (motion,) = clip["motions"]
        self.assertEqual(motion["node"], 0)
        self.assertEqual([len(c) for c in motion["channels"]], [2, 0, 0, 0, 0, 0])
        self.assertEqual(motion["channels"][0][1][:3], [2.0, 5.0, 2])

    def test_a_wrong_tag_is_an_error_not_silent_garbage(self):
        bad = bytearray(scene())
        bad[12:16] = struct.pack("<I", 7)  # the first tag must be Name
        with self.assertRaises(ValueError):
            parse_scene(bytes(bad))

    def test_the_flat_node_list_comes_from_the_strict_reader(self):
        (node,) = parse(scene())
        self.assertEqual(
            (node.name, node.parent, node.pos), ("Smoke1", -1, (1.0, 2.0, 3.0))
        )
        self.assertEqual((node.kind, node.label), ("PlayEffect", "default"))
        self.assertEqual(
            parse(b"\0" * 64), []
        )  # unreadable: the pattern scan finds nothing


if __name__ == "__main__":
    unittest.main()
