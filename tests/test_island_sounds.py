import json
import unittest

from imported import ASSETS, needs_assets


@needs_assets
class IslandSounds(unittest.TestCase):
    def test_every_volcano_hums_and_its_loop_is_imported(self):
        sounds = json.loads((ASSETS / "data/island_sounds.json").read_text())
        self.assertEqual(len(sounds), 7)  # Island_Volcano_01..07
        for name, s in sounds.items():
            self.assertTrue(name.startswith("Island_Volcano_"), name)
            self.assertGreater(s["distance"], 0)
            loop = s["sound"].lower() + ".ogg"
            self.assertTrue(list((ASSETS / "sound").rglob(loop)), f"{loop} missing")


if __name__ == "__main__":
    unittest.main()
