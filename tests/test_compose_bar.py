import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools import compose_bar
from tools.hud_sprites import sprite_dir


def sprite(out, name, skin, size, trim, colour):
    """A synthetic sprite: its visible part (the size less the trim) filled with `colour`."""
    left, top, right, bottom = trim
    art = Image.new("RGBA", (size[0] - left - right, size[1] - top - bottom), colour)
    folder = sprite_dir(out, name)
    (folder / skin).mkdir(parents=True, exist_ok=True)
    art.save(folder / skin / "NoCursor.png")
    (folder / f"{skin}.json").write_text(
        json.dumps({"states": {"NoCursor": {"size": size, "trim": trim}}})
    )


BAR = [
    {"sprite": "A"},
    {"sprite": "B", "backdrop": "MainBar_LeftStretchedDetail"},
    {"sprite": "C", "backdrop": "MainBar_LeftStretchedDetail"},
    {"sprite": "D"},
]
RED, GREEN, BLUE, GREY = (
    (255, 0, 0, 255),
    (0, 255, 0, 255),
    (0, 0, 255, 255),
    (9, 9, 9, 255),
)


class ComposeBar(unittest.TestCase):
    def compose(self, wings):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        out = Path(tmp.name)
        sprite(out, "A", "Navy", [10, 12], [0, 0, 0, 0], RED)
        sprite(out, "B", "Navy", [8, 14], [0, 0, 2 if not wings else 0, 0], GREEN)
        sprite(out, "C", "Navy", [8, 12], [0, 2, 0, 0], BLUE)
        sprite(out, "D", "Navy", [6, 13], [0, 0, 0, 0], RED)
        if wings:
            sprite(
                out, "MainBar_LeftStretchedDetail", "Navy", [4, 12], [0, 0, 0, 11], GREY
            )
        bar = out / "bar.json"
        bar.write_text(json.dumps(BAR))
        compose_bar.main(out, bar)
        data = json.loads((out / "hud/bar/navy.json").read_text())
        return data, Image.open(out / "hud/bar/navy.png")

    def test_parts_butt_exactly_and_the_image_is_the_shortest_part_tall(self):
        data, image = self.compose(wings=False)
        # B and C lose their transparent right columns (B has 2); the bar is as wide as the parts add up, 12 rows tall.
        self.assertEqual(
            [(p["x"], p["width"]) for p in data["parts"]],
            [(0, 10), (10, 6), (16, 8), (24, 6)],
        )
        self.assertEqual((data["width"], data["height"]), (30, 12))
        self.assertEqual(image.size, (30, 12))
        self.assertEqual(
            [image.getpixel((x, 6)) for x in (0, 9, 10, 15, 16, 23, 24, 29)],
            [RED, RED, GREEN, GREEN, BLUE, BLUE, RED, RED],
        )
        # C's top two rows are its transparent trim.
        self.assertEqual(image.getpixel((16, 1))[3], 0)

    def test_skins_with_stretched_backdrops_share_one_edge_pixel(self):
        data, image = self.compose(wings=True)
        self.assertEqual(
            [(p["x"], p["width"]) for p in data["parts"]],
            [(0, 10), (9, 8), (16, 8), (23, 6)],
        )
        self.assertEqual(data["width"], 29)
        # B overlaps A by one pixel, C overlaps B, D overlaps C: the later part wins the shared column.
        self.assertEqual(
            [image.getpixel((x, 6)) for x in (8, 9, 15, 16, 22, 23)],
            [RED, GREEN, GREEN, BLUE, BLUE, RED],
        )

    def test_a_white_last_column_is_left_out_of_the_layout(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            sprite(out, "A", "Navy", [5, 4], [0, 0, 0, 0], (255, 255, 255, 255))
            bar = out / "bar.json"
            bar.write_text(json.dumps([{"sprite": "A"}]))
            compose_bar.main(out, bar)
            self.assertEqual(
                json.loads((out / "hud/bar/navy.json").read_text())["width"], 4
            )


if __name__ == "__main__":
    unittest.main()
