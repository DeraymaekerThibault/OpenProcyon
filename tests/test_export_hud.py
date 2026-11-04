import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from PIL import Image

from tools import hud_sprites
from tools.export_hud import main

# One 12x12 sheet: a 6x4 sprite with a transparent border at (2,3)-(7,6) and a second, differently placed art for the hover state.
HDT = """Specific Resolution ChunkArray - Element
{
\t00000024 MainBar_Test
\t{
\t\tTextureName - NoCursor   ( Navy    ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 0, 0, 7, 6 )
\t\tTextureName - CursorOver ( Navy    ) String 'Sheet_A'
\t\tTextureArea - CursorOver ( Navy    ) Rect( 0, 0, 7, 6 )
\t\tTextureName - CursorDown ( Navy    ) String 'Sheet_A'
\t\tTextureArea - CursorDown ( Navy    ) Rect( 4, 6, 9, 9 )
\t\tTopLeft X                ( Navy    ) Int 5
\t\tTopLeft Y                ( Navy    ) Int 7
\t\tTextureName - NoCursor   ( Pirate  ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Pirate  ) Rect( 0, 0, 7, 6 )
\t}
\t00000025 Empty_Test
\t{
\t\tTextureName - NoCursor   ( Navy    ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 10, 0, 11, 1 )
\t}
\t00000026 Wide_Test
\t{
\t\tTextureName - NoCursor   ( Navy    ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 8, 8, 14, 9 )
\t}
\t00000027 Numbers
\t{
\t\tAnimation Speed          ( Navy    ) Float 0.5
\t}
\t00000028 ../escape
\t{
\t\tTextureName - NoCursor   ( Navy    ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 0, 0, 1, 1 )
\t}
}
"""


def sheet():
    """Sheet with a red 4x3 block at (2,3), a green 2x2 block at (5,7) and white-RGB transparent pixels everywhere else."""
    image = Image.new("RGBA", (12, 12), (255, 255, 255, 0))
    for x in range(2, 6):
        for y in range(3, 6):
            image.putpixel((x, y), (255, 0, 0, 255))
    for x in range(5, 7):
        for y in range(7, 9):
            image.putpixel((x, y), (0, 255, 0, 255))
    return image


def size_of(path):
    with Image.open(path) as image:
        return image.size


def visible(image):
    """Pixels as (r, g, b, a) with the colour of fully transparent ones zeroed: only what can be seen."""
    raw = image.convert("RGBA").tobytes()
    pixels = [tuple(raw[i : i + 4]) for i in range(0, len(raw), 4)]
    return [p if p[3] else (0, 0, 0, 0) for p in pixels]


class ExportHud(unittest.TestCase):
    def export(self, d, image=None):
        root, out = Path(d, "game"), Path(d, "out")
        (root / "BD_GUITextures").mkdir(parents=True, exist_ok=True)
        (root / "Fonts").mkdir(exist_ok=True)
        (root / "hud.hdt").write_text(HDT)
        (image or sheet()).save(root / "BD_GUITextures/Sheet_A.tga")
        with contextlib.redirect_stdout(io.StringIO()):
            main(root, out)
        return root, out

    def test_sprites_are_trimmed_and_described(self):
        with tempfile.TemporaryDirectory() as d:
            _, out = self.export(d)
            base = out / "hud/sprites/MainBar_Test"
            with Image.open(base / "Navy/NoCursor.png") as idle:
                self.assertEqual(idle.size, (4, 3))  # the red block, nothing else
                self.assertEqual(idle.getpixel((0, 0)), (255, 0, 0, 255))
            meta = json.loads((base / "Navy.json").read_text())
            self.assertEqual(
                meta["states"]["NoCursor"], {"size": [8, 7], "trim": [2, 3, 2, 1]}
            )
            self.assertEqual((meta["TopLeft X"], meta["TopLeft Y"]), (5, 7))
            # CursorOver draws like NoCursor: no file, the game falls back to the idle art.
            self.assertNotIn("CursorOver", meta["states"])
            self.assertFalse((base / "Navy/CursorOver.png").exists())
            self.assertEqual(
                meta["states"]["CursorDown"], {"size": [6, 4], "trim": [1, 1, 3, 1]}
            )
            self.assertEqual(size_of(base / "Navy/CursorDown.png"), (2, 2))
            # Other art sets get their own files.
            self.assertTrue((base / "Pirate/NoCursor.png").exists())
            self.assertFalse((base / "Procyon.json").exists())

    def test_trim_offsets_restore_the_original_rectangle(self):
        with tempfile.TemporaryDirectory() as d:
            _, out = self.export(d)
            restored = hud_sprites.read(out, "MainBar_Test", "Navy")
            self.assertEqual(restored.size, (8, 7))
            self.assertEqual(visible(restored), visible(sheet().crop((0, 0, 8, 7))))
            # A state without its own art reads as the idle one.
            self.assertEqual(
                visible(hud_sprites.read(out, "MainBar_Test", "Navy", "CursorOver")),
                visible(restored),
            )
            self.assertIsNone(hud_sprites.read(out, "MainBar_Test", "Procyon"))

    def test_invisible_sprites_keep_a_one_pixel_file_and_outside_rects_are_padded(self):
        with tempfile.TemporaryDirectory() as d:
            _, out = self.export(d)
            empty = out / "hud/sprites/Empty_Test"
            self.assertEqual(size_of(empty / "Navy/NoCursor.png"), (1, 1))
            meta = json.loads((empty / "Navy.json").read_text())["states"]["NoCursor"]
            self.assertEqual(meta["size"], [2, 2])
            # Wide_Test reaches past the 12 px sheet: the part outside it counts as transparent.
            wide = json.loads((out / "hud/sprites/Wide_Test/Navy.json").read_text())[
                "states"
            ]["NoCursor"]
            self.assertEqual(wide["size"], [7, 2])
            self.assertEqual(
                size_of(out / "hud/sprites/Wide_Test/Navy/NoCursor.png"), (1, 1)
            )

    def test_unsafe_names_and_artless_components_get_no_folder(self):
        with tempfile.TemporaryDirectory() as d:
            _, out = self.export(d)
            self.assertFalse((out / "hud/sprites/Numbers").exists())
            self.assertEqual(
                sorted(p.name for p in (out / "hud/sprites").iterdir()),
                ["Empty_Test", "MainBar_Test", "Wide_Test"],
            )

    def test_hud_json_keeps_numbers_but_not_art(self):
        with tempfile.TemporaryDirectory() as d:
            _, out = self.export(d)
            hud = json.loads((out / "hud/hud.json").read_text())
            navy = hud["MainBar_Test"]["Navy"]
            self.assertEqual(navy, {"TopLeft X": 5, "TopLeft Y": 7})
            self.assertEqual(hud["Numbers"]["Navy"], {"Animation Speed": 0.5})

    def test_colour_under_transparent_pixels_comes_from_the_art(self):
        with tempfile.TemporaryDirectory() as d:
            hole = sheet()
            hole.putpixel(
                (3, 4), (255, 255, 255, 0)
            )  # a transparent pixel inside the red block, white underneath
            _, out = self.export(d, hole)
            with Image.open(out / "hud/sprites/MainBar_Test/Navy/NoCursor.png") as idle:
                self.assertEqual(idle.getpixel((1, 1)), (255, 0, 0, 0))

    def test_reimport_replaces_old_atlases_and_sprites(self):
        with tempfile.TemporaryDirectory() as d:
            root, out = self.export(d)
            stale = out / "hud/sprites/Gone/Navy"
            stale.mkdir(parents=True)
            (stale / "NoCursor.png").write_bytes(b"old")
            atlas = out / "hud/tex/sheet_a.png"
            atlas.write_bytes(b"old atlas")
            (out / "hud/tex/bar_navy_mainbar_test.png").write_bytes(b"old bar")
            with contextlib.redirect_stdout(io.StringIO()):
                main(root, out)
            self.assertFalse(stale.parent.exists())
            self.assertFalse(atlas.exists())
            self.assertFalse((out / "hud/tex/bar_navy_mainbar_test.png").exists())
            self.assertTrue(
                (out / "hud/sprites/MainBar_Test/Navy/NoCursor.png").exists()
            )

    def test_font_sheets_are_still_exported(self):
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "BD_GUITextures").mkdir(parents=True)
            (root / "Fonts").mkdir()
            (root / "hud.hdt").write_text(HDT)
            sheet().save(root / "BD_GUITextures/Sheet_A.tga")
            sheet().save(root / "BD_GUITextures/Font_0.tga")
            (root / "Fonts/Test14.fft").write_text("unused")
            font = {"textures": ["Font_0"], "height": 14, "chars": {}}
            with (
                mock.patch("tools.export_hud.parse_font", return_value=font),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                main(root, out)
            self.assertTrue((out / "hud/tex/font_0.png").exists())
            self.assertTrue((out / "hud/fonts/Test14.json").exists())
            self.assertFalse((out / "hud/tex/sheet_a.png").exists())


if __name__ == "__main__":
    unittest.main()
