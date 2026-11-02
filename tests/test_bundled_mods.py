import json
import unittest
from pathlib import Path

MODS = Path(__file__).resolve().parents[1] / "mods"


class BundledMods(unittest.TestCase):
    def test_every_bundled_mod_describes_itself(self):
        folders = [d for d in MODS.iterdir() if d.is_dir()]
        self.assertTrue(folders)
        for d in folders:
            meta = json.loads((d / "mod.json").read_text())
            for key in ("id", "name", "version", "category", "description"):
                self.assertTrue(meta.get(key), f"{d.name}/mod.json: {key}")
            self.assertEqual(meta["id"], d.name)
            self.assertIsInstance(
                meta.get("default", False), bool, f"{d.name}/mod.json: default"
            )


if __name__ == "__main__":
    unittest.main()
