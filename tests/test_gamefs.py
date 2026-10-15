"""Case-insensitive install lookup and shared WOT vector parsing."""

import tempfile
import unittest
from pathlib import Path

from tools.gamefs import ci_file, ci_files
from tools.wot import fields, parse_text


class GameFs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        for name in ("Base_Fort.wot", "base_tower.WOT", "Ship_Cutter.wot"):
            (self.dir / name).write_text("")

    def tearDown(self):
        self.tmp.cleanup()

    def test_glob_ignores_case_and_sorts(self):
        self.assertEqual(
            [p.name for p in ci_files(self.dir, "Base_*.wot")],
            ["Base_Fort.wot", "base_tower.WOT"],
        )

    def test_file_returns_the_real_name_or_raises(self):
        self.assertEqual(ci_file(self.dir, "SHIP_CUTTER.WOT").name, "Ship_Cutter.wot")
        with self.assertRaises(FileNotFoundError):
            ci_file(self.dir, "Ship_Missing.wot")


class Vector(unittest.TestCase):
    def test_reads_signed_and_exponent_components(self):
        nodes, errors = parse_text("Position Vector3( -12.5, 1.5E+03, 1e-05 )")
        self.assertFalse(errors)
        self.assertEqual(fields(nodes)["Position"], [-12.5, 1500.0, 1e-05])


if __name__ == "__main__":
    unittest.main()
