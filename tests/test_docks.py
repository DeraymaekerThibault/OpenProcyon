import json
import unittest
from pathlib import Path

from imported import ASSETS, needs_assets


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


if __name__ == "__main__":
    unittest.main()
