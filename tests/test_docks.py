import json
import struct
import unittest

import numpy as np
from imported import ASSETS, needs_assets


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
        # The node's own position is in the model's frame; adding the parent layer's offset (as gun mounts need) threw
        # Montgomery's docks ~100 units off its piers.
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


if __name__ == "__main__":
    unittest.main()
