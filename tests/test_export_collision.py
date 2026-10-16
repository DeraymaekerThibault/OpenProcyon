import json
import struct
import unittest

from tools.export_collision import definition, triangles


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


if __name__ == "__main__":
    unittest.main()
