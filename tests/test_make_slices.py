import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools import make_slices
from tools.hud_sprites import sprite_dir


def sprite(out, name, skin, size, colour, states=("NoCursor",), trim=(0, 0, 0, 0)):
    left, top, right, bottom = trim
    folder = sprite_dir(out, name)
    (folder / skin).mkdir(parents=True, exist_ok=True)
    info = {}
    for state in states:
        art = Image.new(
            "RGBA", (size[0] - left - right, size[1] - top - bottom), colour
        )
        art.save(folder / skin / f"{state}.png")
        info[state] = {"size": list(size), "trim": list(trim)}
    (folder / f"{skin}.json").write_text(json.dumps({"states": info}))


class MakeSlices(unittest.TestCase):
    def run_slices(self, spec, build):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        out = Path(tmp.name)
        build(out)
        slices = out / "slices.json"
        slices.write_text(json.dumps(spec))
        make_slices.main(out, slices)
        return out / "hud/tex"

    def test_a_group_is_a_json_entry_and_joins_three_pieces_per_state(self):
        def build(out):
            sprite(
                out, "L", "Navy", [5, 8], (255, 0, 0, 255), ("NoCursor", "CursorOver")
            )
            sprite(
                out, "C", "Navy", [3, 8], (0, 255, 0, 255), ("NoCursor", "CursorOver")
            )
            sprite(
                out, "R", "Navy", [4, 8], (0, 0, 255, 255), ("NoCursor", "CursorOver")
            )

        spec = {
            "bar": {
                "left": "L",
                "center": "C",
                "right": "R",
                "states": ["NoCursor", "CursorOver"],
            }
        }
        tex = self.run_slices(spec, build)
        image = Image.open(tex / "slice_bar_navy_cursorover.png")
        self.assertEqual(image.size, (12, 8))
        self.assertEqual(
            [image.getpixel((x, 4)) for x in (0, 4, 5, 7, 8, 11)][::1],
            [(255, 0, 0, 255)] * 2 + [(0, 255, 0, 255)] * 2 + [(0, 0, 255, 255)] * 2,
        )
        self.assertTrue((tex / "slice_bar_navy_nocursor.png").is_file())
        # A skin that lacks the pieces gets no file.
        self.assertFalse((tex / "slice_bar_pirate_nocursor.png").exists())

    def test_two_pieces_are_butted_without_their_transparent_inner_columns(self):
        def build(out):
            sprite(out, "L", "Navy", [10, 4], (255, 0, 0, 255), trim=(0, 0, 1, 0))
            sprite(out, "R", "Navy", [10, 4], (0, 0, 255, 255), trim=(1, 0, 0, 0))

        tex = self.run_slices(
            {"track": {"left": "L", "right": "R", "caps": [3, 3]}}, build
        )
        image = Image.open(tex / "slice_track_navy_nocursor.png")
        self.assertEqual(image.size, (18, 4))
        self.assertEqual([image.getpixel((x, 2))[3] for x in range(18)], [255] * 18)


if __name__ == "__main__":
    unittest.main()
