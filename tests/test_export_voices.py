import json
import tempfile
import unittest
from pathlib import Path

from tools.export_voices import EVENTS, copy


class ExportVoices(unittest.TestCase):
    def test_copy_takes_named_and_crew_post_files_and_reports_missing(self):
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "Sound/Dialogue").mkdir(parents=True)
            # Retail case differs from the table. Every species' crew-post line is copied (the game swaps the prefix);
            # other dialogue (mission lines, AI captains) only when the table names it.
            for n in ["HMCaptain01", "AQSpotter03", "M01Mission01", "HMAICaptain01"]:
                (root / f"Sound/Dialogue/{n}.ogg").write_bytes(b"a")
            events = Path(d, "events.json")
            events.write_text(
                json.dumps(
                    {"HumanMale": {"full_ahead": ["hmcaptain01", "hmcaptain02"]}}
                )
            )
            missing = copy(root, out, events)
            self.assertEqual(
                sorted(p.name for p in (out / "voice").iterdir()),
                ["aqspotter03.ogg", "hmcaptain01.ogg"],
            )
            self.assertEqual(missing, ["hmcaptain02"])

    def test_shipped_table_names_only_files(self):
        table = json.loads(EVENTS.read_text())
        for race, events in table.items():
            for files in events.values():
                self.assertTrue(
                    files and all(f.isascii() and "/" not in f for f in files), race
                )


if __name__ == "__main__":
    unittest.main()
