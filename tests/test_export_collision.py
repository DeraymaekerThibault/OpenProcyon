import json
import struct
import unittest

from tests.imported import ASSETS, needs_assets
from tools.export_collision import bevy, definition, flat_tree, triangles
from tools.mdb import MdbError, box_tree


def tagged(*parts):
    """Bytes of a record stream: ints are u32, floats f32, bytes are kept."""
    out = b""
    for p in parts:
        out += (
            struct.pack("<I", p)
            if isinstance(p, int)
            else struct.pack("<f", p)
            if isinstance(p, float)
            else p
        )
    return out


def block(tag, body):
    return struct.pack("<II", tag, len(body)) + body


def synthetic_node(level, centre, kids=(), tris=()):
    """A node body in the islands' tag numbering."""
    basis = tagged(
        3, *centre, 4, 0, 5, 1.0, 0.0, 0.0, 6, 0.0, 1.0, 0.0, 7, 0.0, 0.0, 1.0
    )
    body = block(2, basis) + tagged(8, 1.0, 2.0, 3.0, 9, 4.0, 10, level)
    body += tagged(11, bytes([len(kids) > 0]), 12, bytes([len(kids) > 1]))
    for tag, kid in zip((13, 16), kids, strict=False):
        body += block(tag, kid)
    if tris:
        body += tagged(14, len(tris)) + b"".join(tagged(15, t) for t in tris)
    return body


class CollisionExportTests(unittest.TestCase):
    def test_ship_box_axes_and_null_detection(self):
        text = """Definition String 'COLLISIONDEFINITION'
DetectionType String 'BoundingRenderEntity'
ResponseType String 'Ship'
MinExtents Vector3( -2, -7, -1.5 )
MaxExtents Vector3( 2, 6.5, 2 )
Definition String 'CUSTOMINFODEFINITION'
MinExtents Vector3( 999, 999, 999 )
"""
        d = definition(text)
        self.assertEqual(d["min"], [-2, -1.5, -6.5])
        self.assertEqual(d["max"], [2, 2, 7])
        self.assertIsNone(definition(text.replace("BoundingRenderEntity", "Null")))
        self.assertIsNone(
            definition("Definition String 'COLLISIONDEFINITION'\nEntityType String ''")
        )

    def test_converted_glb_indexed_triangle(self):
        binary = struct.pack("<9f3I", 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 2, 1)
        doc = dict(
            meshes=[dict(primitives=[dict(attributes={"POSITION": 0}, indices=1)])],
            accessors=[
                dict(bufferView=0, componentType=5126, type="VEC3", count=3),
                dict(bufferView=1, componentType=5125, type="SCALAR", count=3),
            ],
            bufferViews=[dict(byteOffset=0), dict(byteOffset=36)],
        )
        j = json.dumps(doc).encode()
        j += b" " * (-len(j) % 4)
        glb = struct.pack("<III", 0x46546C67, 2, 28 + len(j) + len(binary))
        glb += struct.pack("<II", len(j), 0x4E4F534A) + j
        glb += (
            struct.pack("<II", len(binary), int.from_bytes(b"BIN\0", "little")) + binary
        )
        self.assertEqual(triangles(glb), [[[0, 0, 0], [0, 3, 0], [2, 0, 0]]])

    def test_triangles_follow_the_mesh_node_translation(self):
        binary = struct.pack("<9f3I", 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 2, 1)
        doc = dict(
            meshes=[dict(primitives=[dict(attributes={"POSITION": 0}, indices=1)])],
            nodes=[dict(mesh=0, translation=[0, 7.05, 0])],
            accessors=[
                dict(bufferView=0, componentType=5126, type="VEC3", count=3),
                dict(bufferView=1, componentType=5125, type="SCALAR", count=3),
            ],
            bufferViews=[dict(byteOffset=0), dict(byteOffset=36)],
        )
        j = json.dumps(doc).encode()
        j += b" " * (-len(j) % 4)
        glb = struct.pack("<III", 0x46546C67, 2, 28 + len(j) + len(binary))
        glb += struct.pack("<II", len(j), 0x4E4F534A) + j
        glb += (
            struct.pack("<II", len(binary), int.from_bytes(b"BIN\0", "little")) + binary
        )
        self.assertEqual(triangles(glb), [[[0, 7.05, 0], [0, 10.05, 0], [2, 7.05, 0]]])

    def test_axis_conversion_is_a_rotation_to_y_up(self):
        self.assertEqual(bevy([1, 2, 3]), [1, 3, -2])
        # x cross y = z must stay true after the conversion (no mirroring)
        x, y, z = bevy([1, 0, 0]), bevy([0, 1, 0]), bevy([0, 0, 1])
        cross = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ]
        self.assertEqual(cross, z)

    def test_box_tree_reader_on_a_synthetic_stream(self):
        leaf_a = synthetic_node(1, (1.0, 2.0, 3.0), tris=[7, 9])
        leaf_b = synthetic_node(1, (-1.0, 0.0, 0.0))
        root = synthetic_node(0, (0.0, 0.5, 0.0), kids=[leaf_a, leaf_b])
        trailing = bytes(range(48)) + block(1, root) + tagged(17, 1)
        tree = box_tree(trailing)
        self.assertEqual(tree["level"], 0)
        self.assertEqual(tree["centre"], (0.0, 0.5, 0.0))
        self.assertEqual(
            tree["axes"], [(1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)]
        )
        self.assertEqual(tree["half"], (1.0, 2.0, 3.0))
        self.assertEqual(tree["tris"], [])
        a, b = tree["children"]
        self.assertEqual(
            (a["centre"], a["tris"], a["children"]), ((1.0, 2.0, 3.0), [7, 9], [])
        )
        self.assertEqual((b["centre"], b["tris"]), ((-1.0, 0.0, 0.0), []))
        nodes = flat_tree(tree)
        self.assertEqual([n["k"] for n in nodes], [[1, 2], [], []])
        self.assertEqual(nodes[1]["c"], [1.0, 3.0, -2.0])
        self.assertEqual(nodes[1]["t"], [7, 9])
        self.assertIsNone(box_tree(bytes(64)))
        with self.assertRaises(MdbError):
            box_tree(bytes(48) + block(1, tagged(2, 0)) + b"\0" * 8)

    def test_box_tree_reader_uses_the_files_own_tag_table(self):
        # a mesh whose root is a leaf has no child tags: triangle count is tag 13 and elements tag 14
        names = [
            "MeshData", "Root", "LocalBasis", "Position", "LookAt Vector Length", "Orientation - Cross",
            "Orientation - Forward", "Orientation - Up", "Length", "Radius", "Level", "HasLeftChild", "HasRightChild",
            "Valid Collision Triangle Indices - Size", "Valid Collision Triangle Indices - Element", "MaxLevel",
        ]  # fmt: skip
        table = tagged(len(names)) + b"".join(
            tagged(len(n)) + n.encode() for n in names
        )
        basis = tagged(
            3, 0.0, 0.0, 0.0, 4, 0, 5, 1.0, 0.0, 0.0, 6, 0.0, 1.0, 0.0, 7, 0.0, 0.0, 1.0
        )
        body = block(2, basis) + tagged(8, 1.0, 1.0, 1.0, 9, 1.0, 10, 0)
        body += tagged(11, b"\0", 12, b"\0", 13, 2, 14, 5, 14, 6)
        tree = box_tree(bytes(48) + block(1, body) + tagged(15, 0) + table)
        self.assertEqual(tree["tris"], [5, 6])
        self.assertEqual(tree["children"], [])

    @needs_assets
    def test_imported_island_trees(self):
        doc = json.loads((ASSETS / "data/collision.json").read_text())
        if "layers" not in doc.get("Island_Rocky_08", {}):
            self.skipTest("collision data imported before box trees were exported")
        (layer,) = doc["Island_Rocky_08"]["layers"]
        nodes = layer["nodes"]
        self.assertEqual(len(nodes), 47)
        self.assertEqual(sorted({n["l"] for n in nodes}), [0, 1, 2, 3, 4, 5])
        self.assertEqual(sum(1 for n in nodes if not n["k"]), 24)
        # game z +7.05 is remake y
        self.assertAlmostEqual(layer["offset"][1], 7.05, places=3)
        self.assertEqual(layer["offset"][0], 0)
        (layer6,) = doc["Island_Rocky_06"]["layers"]
        self.assertEqual(layer6["offset"], [0, 0, 0])
        self.assertEqual(len(layer6["nodes"]), 47)


if __name__ == "__main__":
    unittest.main()
