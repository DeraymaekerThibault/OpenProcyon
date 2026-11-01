"""Importer must not abort on incomplete installs or malformed mod files. All inputs are synthetic."""

import struct
import tempfile
import unittest
from pathlib import Path

from tools import export_saves
from tools.export_campaign import parse_mission
from tools.gamefs import ci_files
from tools.mdb import MdbError
from tools.mdb import parse as parse_mdb
from tools.wot import parse_text

RULES = "00000001 World Rules\n{\n}\n"


class ImportTolerance(unittest.TestCase):
    def test_missing_directory_is_empty(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(ci_files(Path(d) / "SavedGames", "*.twt"), [])

    def saves(self, files):
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d) / "game", Path(d) / "out"
            for name in files:
                (root / "SavedGames").mkdir(parents=True, exist_ok=True)
                (root / "SavedGames" / name).write_text("")
            root.mkdir(exist_ok=True)
            export_saves.main(root, out)
            return sorted(p.name for p in (out / "saves").iterdir())

    def test_saves_without_any_snapshot(self):
        self.assertIn("mission13.json", self.saves([]))

    def test_saves_without_mission_12(self):
        self.assertIn("mission13.json", self.saves(["Mission 05.twt"]))

    def test_nine_digit_block_header(self):
        _, diagnostics = parse_text("000000059 Group\n{\n}\n", "x.twt")
        self.assertEqual(diagnostics, [])

    def test_stray_closing_brace_is_tolerated(self):
        parse_mission(RULES + "}\n}\n")

    def test_truncated_mdb_raises_mdb_error(self):
        with self.assertRaises(MdbError):
            parse_mdb(struct.pack("<5I", 0, 0, 0, 1, 100))


if __name__ == "__main__":
    unittest.main()
