import json
import os
import struct
import unittest

from tools.mdb import MdbError, parse


def rec(payload):
    return struct.pack("<I", len(payload)) + payload


def synthetic(bad_index=False):
    verts = b"".join(
        rec(
            struct.pack("<8f", x, 0, 0, 0, 0, 0, 0, 0)[:28]
            + struct.pack("<I", 0xFFFFFFFF)
        )
        for x in (0, 1, 2)
    )
    faces = rec(struct.pack("<4H", 0, 1, 9 if bad_index else 2, 0))
    model = (
        struct.pack("<II", 7, 3)
        + verts
        + struct.pack("<I", 1)
        + faces
        + struct.pack("<I", 0)
    )
    name = b"wood.tga\0"
    mat = rec(struct.pack("<I", len(name)) + name + b"\0" * 72)
    return struct.pack("<4I", 0, 0, 0, 1) + rec(model) + struct.pack("<I", 1) + mat


class MdbTests(unittest.TestCase):
    def test_synthetic(self):
        m = parse(synthetic())
        self.assertEqual(m.textures, ["wood.tga"])
        self.assertEqual(m.models[0].id, 7)
        self.assertEqual([v.pos[0] for v in m.models[0].vertices], [0, 1, 2])
        self.assertEqual(m.models[0].faces, [(0, 1, 2, 0)])

    def test_rejects_bad_index_and_truncation(self):
        with self.assertRaises(MdbError):
            parse(synthetic(bad_index=True))
        with self.assertRaises(MdbError):
            parse(synthetic()[:-5])

    def test_packed_normal(self):
        import math

        from tools.mdb import Vertex, normal

        def n(az, el):
            return [
                round(c, 6) + 0.0
                for c in normal(Vertex((0, 0, 0), (0, 0), (az, el), 0))
            ]

        self.assertEqual(n(0, 0), [0, 1, 0])
        self.assertEqual(n(math.pi / 2, 0), [-1, 0, 0])
        self.assertEqual(n(0, math.pi / 2), [0, 0, 1])


@unittest.skipUnless(os.environ.get("PROCYON_GAME_ROOT"), "needs PROCYON_GAME_ROOT")
class LocalMdbTests(unittest.TestCase):
    def test_cutter_glb(self):
        from tools.mdb_to_glb import convert

        glb = convert(os.environ["PROCYON_GAME_ROOT"], "NavyCutterShip")
        n = struct.unpack_from("<I", glb, 12)[0]
        g = json.loads(glb[20 : 20 + n])
        self.assertEqual(
            len(g["meshes"]), 12
        )  # 6 layers + their retail-named Damage_ swap meshes
        self.assertEqual(len(g["images"]), 31)
        self.assertTrue(
            all(
                "NORMAL" in p["attributes"]
                for m in g["meshes"]
                for p in m["primitives"]
            )
        )


if __name__ == "__main__":
    unittest.main()
