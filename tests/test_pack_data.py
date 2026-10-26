import json
import tempfile
import unittest
from pathlib import Path

from tools.pack_data import pack

GUN = {"MuzzleSpeed": 40.0}
RAW = {
    "entity": "Ship_X",
    "scene": "X",
    "race": "Navy",
    "core_hp": 10,
    "physics": {"Mass": {"value": 5.0, "line": 3}},
    "sections": [
        {
            "layer": "X_Layer1",
            "label": "bow",
            "hp": 1.0,
            "aabb": [[0, 0, 0], [1, 1, 1]],
            "tris": [0.123456789] * 9,
        }
    ],
    "guns": [{"label": "Port1", "gun": "G", "pos": [0.5, 1.0, 2.0]}],
    "gun_defs": {"G": GUN},
    "bullets": {"B": {"lifetime": 1.0}},
    "engines": [],
    "flags": [],
}


class PackData(unittest.TestCase):
    def test_split_layout(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "Ship_X.json").write_text(json.dumps(RAW))
            pack(d)
            d = Path(d)
            self.assertFalse((d / "Ship_X.json").exists())
            ship = json.loads((d / "data/ships/Ship_X.json").read_text())
            self.assertEqual(ship["physics"], {"Mass": 5.0})
            self.assertFalse(ship["skirmish"])
            self.assertEqual(list(ship["sections"]), ["bow"])
            self.assertNotIn("tris", ship["sections"]["bow"])
            self.assertEqual(ship["guns"]["Port1"]["gun"], "G")
            geo = json.loads((d / "data/geometry/Ship_X.json").read_text())
            self.assertEqual(geo["X_Layer1"]["tris"][0], 0.1235)
            self.assertEqual(
                json.loads((d / "data/weapons/guns.json").read_text()), {"G": GUN}
            )

    def test_conflicting_weapon_is_an_error(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "Ship_A.json").write_text(json.dumps(RAW))
            (Path(d) / "Ship_B.json").write_text(
                json.dumps({**RAW, "gun_defs": {"G": {"MuzzleSpeed": 1.0}}})
            )
            with self.assertRaises(AssertionError):
                pack(d)


if __name__ == "__main__":
    unittest.main()
