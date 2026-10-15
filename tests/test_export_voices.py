import json
import tempfile
import unittest
from pathlib import Path

from tools.export_voices import EVENTS, copy


class ExportVoices(unittest.TestCase):
    def test_copy_takes_named_retail_files_and_reports_missing(self):
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "Sound/Dialogue").mkdir(parents=True)
            # Retail case differs from the table; HMCaptain99 is not in it, so it is not copied.
            (root / "Sound/Dialogue/HMCaptain01.ogg").write_bytes(b"a")
            (root / "Sound/Dialogue/HMCaptain99.ogg").write_bytes(b"b")
            events = Path(d, "events.json")
            events.write_text(
                json.dumps(
                    {"HumanMale": {"full_ahead": ["hmcaptain01", "hmcaptain02"]}}
                )
            )
            missing = copy(root, out, events)
            self.assertEqual(
                sorted(p.name for p in (out / "voice").iterdir()), ["hmcaptain01.ogg"]
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
