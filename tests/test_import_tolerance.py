"""Importer must not abort on incomplete installs or malformed mod files. All inputs are synthetic."""

import struct
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest import mock

from tools import export_combat, export_saves
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

    def test_gun_with_unreadable_model_leaves_no_definition(self):
        # Two mounts of one gun whose model fails: the second must not reuse a half-built (visual-less) definition.
        mounts = [("m", "A", "Gun_X,90,Port"), ("m", "B", "Gun_X,90,Port")]
        with mock.patch.multiple(
            export_combat,
            load=mock.DEFAULT,
            scene_name=mock.Mock(return_value="S"),
            hardpoints=mock.Mock(return_value=[]),
            parse_msb=mock.Mock(return_value=[]),
            ci_file=mock.DEFAULT,
            associations=lambda _, kind: mounts if kind == "GunPlacement" else [],
            flat=lambda _: defaultdict(float, {"BulletTypeName_Primary": "B"}),
            gun_visual=mock.Mock(side_effect=MdbError("truncated")),
            base_safe_physics=mock.Mock(return_value={}),
            lifeboat_fields=mock.Mock(return_value={}),
        ):
            j = export_combat.export("root", "Ship_X", "out")
        self.assertEqual(j["gun_defs"], {})
        self.assertEqual(j["guns"], [])
        self.assertEqual(len(j["skipped_guns"]), 2)


if __name__ == "__main__":
    unittest.main()
