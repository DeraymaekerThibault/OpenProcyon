import os
import struct
import tempfile
import unittest
from pathlib import Path

from tools.msb import parse


def s(text):
    return struct.pack("<I", len(text)) + text.encode()


def node(nid, parent, name, floats, kind="", label=""):
    b = struct.pack("<IIIiII", 2, nid, 5, parent, 6, 0) + struct.pack("<I", 1) + s(name)
    b += struct.pack("<I3I", 7, 0, 0, 0)
    b += b"".join(struct.pack("<If", 8, f) for f in floats)
    b += struct.pack("<II", 9, 1)
    if kind:
        b += (
            struct.pack("<II", 10, 5)
            + struct.pack("<I", 11)
            + s(kind)
            + struct.pack("<I", 12)
            + s(label)
        )
    return b


class MsbTests(unittest.TestCase):
    def test_nodes_and_noise(self):
        data = (
            b"\xaa" * 20
            + node(1, -1, "root", [0] * 6)
            + b"\x00" * 8
            + node(2, 1, "m", [1, 2, 3, 1.5, 0, 0], "GunPlacement", "Port_X")
        )
        a, b = parse(data)
        self.assertEqual((a.id, a.parent, a.kind), (1, -1, ""))
        self.assertEqual(
            (b.id, b.parent, b.pos, b.kind, b.label),
            (2, 1, (1.0, 2.0, 3.0), "GunPlacement", "Port_X"),
        )
        self.assertAlmostEqual(b.rot[0], 1.5)


class NestedChild(unittest.TestCase):
    def test_child_nested_in_the_kind_block_is_its_own_node(self):
        # Vanilla AsteroidXlarge: the root's 0a/0b block holds the layer node instead of a kind string.
        child = node(2, 1, "ASTEROIDS_XLarge_Layer1", [0] * 6)
        block = (
            struct.pack("<II", 10, 5)
            + struct.pack("<I", 11)
            + s(child.decode("latin-1"))
            + struct.pack("<I", 12)
            + s("")
        )
        root = node(1, -1, "XLargeAsteroid_NULL", [0] * 6) + block
        names = [(n.name, n.kind) for n in parse(root)]
        self.assertEqual(
            names, [("XLargeAsteroid_NULL", ""), ("ASTEROIDS_XLarge_Layer1", "")]
        )


class SceneLayers(unittest.TestCase):
    def test_layers_named_by_the_scene_when_the_prefix_differs(self):
        # Vanilla Silver: scene PirateSilverShip, meshes JohnSilverShip_Layer*.mdb (Damage variants are added later by convert).
        from tools.mdb_to_glb import scene_layers

        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "BD_MeshScenes").mkdir()
            (root / "BD_Meshes").mkdir()
            names = [
                "JohnSilverShip_Layer10",
                "JohnSilverShip_Layer2",
                "JohnSilverShipDamage_Layer2",
                "JohnSilverShip_Layer2",
                "Gun_Null",
            ]
            msb = b"".join(node(i + 1, -1, n, [0] * 6) for i, n in enumerate(names))
            (root / "BD_MeshScenes/PirateSilverShip.msb").write_bytes(msb)
            for n in (
                "JohnSilverShip_Layer2",
                "JohnSilverShip_Layer10",
                "JohnSilverShipDamage_Layer2",
            ):
                (root / "BD_Meshes" / f"{n}.mdb").write_bytes(b"")
            got = [p.name for p in scene_layers(root, "PirateSilverShip")]
        self.assertEqual(
            got, ["JohnSilverShip_Layer2.mdb", "JohnSilverShip_Layer10.mdb"]
        )


@unittest.skipUnless(os.environ.get("PROCYON_GAME_ROOT"), "needs PROCYON_GAME_ROOT")
class LocalMsbTests(unittest.TestCase):
    def test_cutter_hardpoints(self):
        from pathlib import Path

        from tools.mdb_to_glb import hardpoints

        hp = hardpoints(os.environ["PROCYON_GAME_ROOT"], "NavyCutterShip")
        from tools.msb import parse as P

        ids = [
            n.id
            for n in P(
                (
                    Path(os.environ["PROCYON_GAME_ROOT"])
                    / "BD_MeshScenes/NavyCutterShip.msb"
                ).read_bytes()
            )
        ]
        self.assertEqual(ids, list(range(26)))
        self.assertEqual(sum(n.kind == "GunPlacement" for n, _ in hp), 10)
        mid = next(p for n, p in hp if n.label == "Port_Medmount1")
        self.assertLess(mid[0], 0)  # port is -x


if __name__ == "__main__":
    unittest.main()
