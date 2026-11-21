import json
import struct
import unittest

import numpy as np
from imported import ASSETS, needs_assets

from tools.mdb_to_glb import with_layer_offset
from tools.msb import Node


def glb_xz(path):
    """Vertices of a GLB in the game's (x, y) plane: bevy x and -z."""
    b = path.read_bytes()
    n = struct.unpack("<I", b[12:16])[0]
    g = json.loads(b[20 : 20 + n])
    start = 20 + n + 8
    pts = []
    for m in g["meshes"]:
        for pr in m["primitives"]:
            a = g["accessors"][pr["attributes"]["POSITION"]]
            v = g["bufferViews"][a["bufferView"]]
            off = start + v.get("byteOffset", 0) + a.get("byteOffset", 0)
            pts.append(np.frombuffer(b, "<f4", a["count"] * 3, off).reshape(-1, 3))
    xyz = np.concatenate(pts)
    return np.stack([xyz[:, 0], -xyz[:, 2]], axis=1)


def glb_xyz(path):
    """Vertices of a GLB's undamaged layers placed by their node translation, in the game's (x, y, z)."""
    b = path.read_bytes()
    n = struct.unpack("<I", b[12:16])[0]
    g = json.loads(b[20 : 20 + n])
    start = 20 + n + 8
    pts = []
    for node in g["nodes"]:
        if "mesh" not in node or "Damage" in node["name"]:
            continue
        for pr in g["meshes"][node["mesh"]]["primitives"]:
            a = g["accessors"][pr["attributes"]["POSITION"]]
            v = g["bufferViews"][a["bufferView"]]
            off = start + v.get("byteOffset", 0) + a.get("byteOffset", 0)
            xyz = np.frombuffer(b, "<f4", a["count"] * 3, off).reshape(-1, 3)
            pts.append(xyz + node.get("translation", [0, 0, 0]))
    xyz = np.concatenate(pts)
    return np.stack([xyz[:, 0], -xyz[:, 2], xyz[:, 1]], axis=1)


class DockExportTests(unittest.TestCase):
    @needs_assets
    def test_exported_dock_points(self):
        d = json.loads(
            (ASSETS / "data/docks.json").read_text()
        )  # bases and islands; ships keep theirs in ships/<id>/docks.json
        d.update(
            {
                f.parent.name: json.loads(f.read_text())
                for f in ASSETS.glob("ships/*/docks.json")
            }
        )
        tender = d["Ship_Navy_Tender"]
        self.assertEqual(
            [p["label"] for p in tender["points"]], ["Port_DockPoint", "Star_DockPoint"]
        )
        self.assertLess(tender["points"][0]["pos"][0], 0)  # port is -x, starboard +x
        self.assertGreater(tender["points"][1]["pos"][0], 0)
        self.assertTrue(d["Base_Grant"]["points"])
        self.assertTrue(tender["tender"])
        self.assertFalse(d["Ship_Navy_Frigate"]["tender"])
        # Points are the MSB 'DockPoint' nodes, so WOTs that label every point 'default' still get distinct sides.
        escort = [p["pos"][0] for p in d["Ship_Navy_Heavy_Escort"]["points"]]
        self.assertTrue(min(escort) < 0 < max(escort), escort)
        self.assertEqual(
            len({tuple(p["pos"]) for p in d["Base_Procyon1"]["points"]}), 4
        )
        ship = json.loads((ASSETS / "ships/Ship_Navy_Tender/ship.json").read_text())
        self.assertEqual(ship["repair_fx"], "Repair_Medium")
        self.assertEqual(ship["sight"], 500.0)
        for name, v in d.items():
            self.assertTrue(name.startswith(("Ship_", "Base_", "Island_")))
            for p in v["points"]:
                self.assertEqual(len(p["pos"]), 3)
                self.assertIsInstance(p["yaw"], float)

    @needs_assets
    def test_base_dock_points_lie_beside_the_model(self):
        # The node's own position is in the model's frame; adding the parent layer's position threw Montgomery's docks
        # ~100 units off its piers.
        # Bases and islands are in data/docks.json; ships have their own ships/<Id>/docks.json (as the game reads them).
        d = json.loads((ASSETS / "data/docks.json").read_text())
        for name, scene in [
            ("Base_Montgomery", "BaseMontgomery"),
            ("Base_Grant", "BaseGrant"),
            ("Base_Procyon1", "BaseProcyons"),
        ]:
            xy = glb_xz(next((ASSETS / "models").rglob(f"{scene}.glb")))
            for p in d[name]["points"]:
                gap = np.hypot(*(xy - np.array(p["pos"][:2])).T).min()
                self.assertLess(gap, 35, f"{name} {p['label']}")

    def test_a_point_moves_with_its_mesh_layer(self):
        # Island_Large01: the layer's scene node sits at 0 with pivot z 29.25, so its mesh hangs 29.25 below the
        # object; the dock under it at z 29.25 comes down to the flight plane with it.
        nodes = {
            3: Node(
                3,
                0,
                "IslandLarge01_Layer2",
                (4.96, -41.57, -0.48),
                (0, 0, 0),
                "DamageSection",
                "",
            ),
            1: Node(
                1,
                3,
                "DockPoint",
                (-22.07, -65.0, 29.25),
                (0, 0, 0),
                "DockPoint",
                "default",
            ),
            2: Node(
                2,
                0,
                "Top_Flag",
                (1.0, 2.0, 3.0),
                (0, 0, 0),
                "FlagAttachmentPoint",
                "Top_Flag",
            ),
        }
        offsets = {"islandlarge01_layer2": (19.71, -8.57, -29.24)}
        self.assertEqual(
            with_layer_offset(offsets, nodes, nodes[1]),
            (-22.07 + 19.71, -65.0 - 8.57, 29.25 - 29.24),
        )
        self.assertEqual(
            with_layer_offset(offsets, nodes, nodes[2]), (1.0, 2.0, 3.0)
        )  # not under an offset layer

    @needs_assets
    def test_large_island_dock_is_at_the_flight_plane(self):
        dock = json.loads((ASSETS / "data/docks.json").read_text())["Island_Large01"][
            "points"
        ][0]
        self.assertLess(abs(dock["pos"][2]), 0.05)

    @needs_assets
    def test_base_boarding_points_lie_on_the_model(self):
        # Like the docks: the node's own position moved with its layer. The scene chain minus the first section
        # left these points 6 to 17 off the model, mostly below it.
        d = json.loads((ASSETS / "data/boarding.json").read_text())
        for name, scene in [
            ("Base_Regina", "BaseRegina"),
            ("Base_Montgomery", "BaseMontgomery"),
        ]:
            xyz = glb_xyz(next((ASSETS / "models").rglob(f"{scene}.glb")))
            for p in d[name]["points"]:
                gap = np.linalg.norm(xyz - np.array(p), axis=1).min()
                self.assertLess(gap, 5, f"{name} {p}")


if __name__ == "__main__":
    unittest.main()
