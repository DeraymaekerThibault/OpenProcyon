import contextlib
import io
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.export_hud import main

HDT = """Specific Resolution ChunkArray - Element
{
\t00000024 MainBar_Test
\t{
\t\tTextureName - NoCursor   ( Navy    ) String 'Sheet_A'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 0, 0, 3, 3 )
\t}
}
"""


class ExportHud(unittest.TestCase):
    def test_reimport_restores_sheets_changed_by_later_steps(self):
        # trim_hud_rects and fix_alpha_bleed edit the exported sheets in place; a second import into the same folder must
        # start again from the retail art, or the bar crops come out different from a fresh import.
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "BD_GUITextures").mkdir(parents=True)
            (root / "Fonts").mkdir()
            (root / "hud.hdt").write_text(HDT)
            Image.new("RGBA", (4, 4), (255, 255, 255, 0)).save(
                root / "BD_GUITextures/Sheet_A.tga"
            )
            sheet = out / "hud/tex/sheet_a.png"
            with contextlib.redirect_stdout(io.StringIO()):
                main(root, out)
                Image.new("RGBA", (4, 4), (9, 9, 9, 0)).save(sheet)
                main(root, out)
            self.assertEqual(Image.open(sheet).getpixel((1, 1)), (255, 255, 255, 0))


if __name__ == "__main__":
    unittest.main()
