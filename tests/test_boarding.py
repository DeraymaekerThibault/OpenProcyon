import json
import unittest

from imported import ASSETS, needs_assets


class BoardingExportTests(unittest.TestCase):
    @needs_assets
    def test_crews_and_boarding_points(self):
        d = json.loads((ASSETS / "data/boarding.json").read_text())
        # Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts (WOT 'Max number of ...')
        self.assertEqual(d["Ship_Pirate_Schooner"]["crew"], [4, 1, 1, 1, 0, 2, 2, 1])
        self.assertEqual(len(d["Ship_Pirate_Schooner"]["points"]), 2)  # BoardingNode0/1
        self.assertEqual(len(d["Base_QueensPalace"]["points"]), 4)
        self.assertNotIn("crew", d["Base_QueensPalace"])
        for name, v in d.items():
            self.assertTrue(name.startswith(("Ship_", "Base_", "Island_")))
            if name.startswith("Ship_"):
                self.assertEqual(len(v["crew"]), 8)
            for p in v["points"]:
                self.assertEqual(len(p), 3)
        # every effect the game spawns for a fight and a grapple was exported with the rest
        index = json.loads((ASSETS / "fx/index.json").read_text())
        self.assertIn("Boarding_Fight", index)
        self.assertTrue((ASSETS / "fx/Boarding_Fight.json").is_file())
