import json
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from tools.trim_hud_rects import main, trimmed_rect


class TrimHudRects(unittest.TestCase):
    def test_exports_isolated_bar_art_without_changing_other_widgets(self):
        with tempfile.TemporaryDirectory() as folder:
            assets = Path(folder)
            (assets / "hud/tex").mkdir(parents=True)
            (assets / "data/hud").mkdir(parents=True)
            (assets / "data/hud/bar.json").write_text('[{"sprite":"MainBar_Test"}]')
            image = Image.new("RGBA", (5, 3))
            image.putpixel((2, 1), (255, 0, 0, 255))
            image.save(assets / "hud/tex/sheet.png")
            sprite = {
                "TextureName - NoCursor": "sheet",
                "TextureArea - NoCursor": [0, 0, 4, 2],
            }
            (assets / "hud/hud.json").write_text(
                json.dumps(
                    {
                        "MainBar_Test": {"Pirate": sprite, "Navy": sprite},
                        "Radar": {"Pirate": sprite},
                    }
                )
            )
            main(assets, assets / "data/hud/bar.json")
            hud = json.loads((assets / "hud/hud.json").read_text())
            self.assertNotIn("TextureAreaTrimmed - NoCursor", hud["Radar"]["Pirate"])
            self.assertEqual(
                hud["MainBar_Test"]["Pirate"]["TextureArea - NoCursor"], [0, 0, 4, 2]
            )
            with Image.open(assets / "hud/tex/bar_pirate_mainbar_test.png") as cropped:
                self.assertEqual(cropped.size, (3, 3))
                self.assertEqual(cropped.getpixel((2, 1)), (255, 0, 0, 255))
            with Image.open(assets / "hud/tex/bar_navy_mainbar_test.png") as cropped:
                self.assertEqual(cropped.size, (5, 3))

    def test_only_trailing_empty_columns_are_trimmed(self):
        alpha = Image.new("L", (5, 3))
        alpha.putpixel((2, 1), 1)  # even nearly transparent artwork must survive
        self.assertEqual(trimmed_rect(alpha, [0, 0, 4, 2]), [0, 0, 2, 2])
        self.assertEqual(trimmed_rect(alpha, [0, 0, 2, 2]), [0, 0, 2, 2])
        self.assertEqual(trimmed_rect(alpha, [0, 0, 4, 3]), [0, 0, 4, 3])
        self.assertEqual(
            trimmed_rect(Image.new("L", (5, 3)), [0, 0, 4, 2]), [0, 0, 0, 2]
        )
