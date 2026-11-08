import re
import unittest
from pathlib import Path

import numpy as np

from tools.bake_surfaces import SLOPE_GAIN, bake, map_name

REPO = Path(__file__).resolve().parent.parent


class BakeSurfaces(unittest.TestCase):
    def test_flat_texture_is_flat(self):
        m = bake(np.full((16, 16, 3), 128, np.uint8))
        self.assertTrue((m[..., :3] == [128, 128, 0]).all())

    def test_ramp_slopes_up_toward_brighter_texels(self):
        # brighter toward +u (columns); wraps, so look away from the seam
        rgb = np.repeat(
            np.linspace(40, 200, 32).astype(np.uint8)[None, :, None], 32, 0
        ).repeat(3, 2)
        m = bake(rgb).astype(int)
        self.assertGreater(m[16, 16, 0], 128)
        self.assertEqual(m[16, 16, 1], 128)

    def test_dark_pit_is_a_crevice(self):
        rgb = np.full((32, 32, 3), 180, np.uint8)
        rgb[15:17, 15:17] = 20
        m = bake(rgb)
        self.assertGreater(m[16, 16, 2], 100)
        self.assertEqual(m[0, 0, 2], 0)

    def test_shader_decodes_with_the_same_gain(self):
        wesl = (REPO / "mods/remaster/shaders/surface.wesl").read_text()
        self.assertEqual(
            float(re.search(r"const SLOPE_GAIN: f32 = ([\d.]+);", wesl)[1]), SLOPE_GAIN
        )

    def test_map_name_matches_the_game(self):
        self.assertEqual(map_name("Rock Fissure.tga"), "rock fissure")
        self.assertEqual(map_name("IslandGrass_3"), "islandgrass_3")


if __name__ == "__main__":
    unittest.main()
