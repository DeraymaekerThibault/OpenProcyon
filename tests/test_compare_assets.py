import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.compare_assets import compare


class CompareAssets(unittest.TestCase):
    def test_meaningful_differences_only(self):
        with tempfile.TemporaryDirectory() as d:
            exp, act = Path(d, "e"), Path(d, "a")
            for root in (exp, act):
                root.mkdir()
            # Same JSON value, different formatting and float noise: equal.
            (exp / "same.json").write_text(
                json.dumps({"a": 1.0, "b": [1, 2]}, indent=2)
            )
            (act / "same.json").write_text(json.dumps({"b": [1, 2], "a": 1.00000001}))
            # A real value change: reported with its path.
            (exp / "value.json").write_text('{"ship": {"hp": 100}}')
            (act / "value.json").write_text('{"ship": {"hp": 120}}')
            # Same pixels, different encoding: equal. Changed pixel: reported.
            img = Image.new("RGBA", (2, 2), (10, 20, 30, 255))
            img.save(exp / "same.png", compress_level=0)
            img.save(act / "same.png", compress_level=9)
            img.save(exp / "pix.png")
            changed = img.copy()
            changed.putpixel((0, 0), (0, 0, 0, 0))
            changed.save(act / "pix.png")
            (exp / "only_e.ogg").write_bytes(b"x")
            (act / "only_a.ogg").write_bytes(b"y")
            r = compare(exp, act)
        self.assertEqual(r["only_expected"], ["only_e.ogg"])
        self.assertEqual(r["only_actual"], ["only_a.ogg"])
        self.assertEqual(sorted(r["differ"]), ["pix.png", "value.json"])
        self.assertIn("/ship/hp 100 != 120", r["differ"]["value.json"])


if __name__ == "__main__":
    unittest.main()
