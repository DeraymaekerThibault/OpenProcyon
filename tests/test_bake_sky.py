import tempfile
import unittest
from pathlib import Path

import numpy as np

from tools.bake_sky import (
    BANDS,
    STAR_GAIN,
    FaceBase,
    SkyGuide,
    azimuth_elevation,
    bake,
    bake_layers,
    draw_stars,
    face_dirs,
    fit_mapping,
    read_png16,
    shade_face,
    write_png16,
)


def uv_sphere(repeats, offset, slope, centre, rings=9, segments=16):
    """A synthetic sky sphere laid out like the imported ones (no game data)."""
    pos, uv = [], []
    for i in range(rings):
        el = np.radians(-80 + 160 * i / (rings - 1))
        for j in range(segments + 1):
            az = np.radians(-180 + 360 * j / segments)
            d = np.array(
                [np.cos(el) * np.sin(az), np.sin(el), -np.cos(el) * np.cos(az)]
            )
            pos.append(d * 3)
            uv.append([offset - repeats * np.degrees(az) / 360, slope * d[1] + centre])
    return np.array(pos, np.float32), np.array(uv, np.float32)


class BakeSky(unittest.TestCase):
    def test_fit_recovers_how_the_sphere_lays_out_the_art(self):
        for repeats in (1, 2, 3):
            pos, uv = uv_sphere(repeats, 0.25, -0.2, 0.5)
            r, offset, slope, centre, err = fit_mapping(pos, uv)
            self.assertEqual(r, repeats)
            self.assertAlmostEqual(offset, 0.25, places=3)
            self.assertAlmostEqual(slope, -0.2, places=3)
            self.assertAlmostEqual(centre, 0.5, places=3)
            self.assertLess(err, 1e-3)

    def test_face_centres_follow_bevys_cube_layout(self):
        # Bevy samples the cube with the world ray times (1, 1, -1): its +Z face is world -Z (ahead)
        want = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, -1), (0, 0, 1)]
        for face, axis in enumerate(want):
            d = face_dirs(face, 2).mean((0, 1))
            np.testing.assert_allclose(d / np.linalg.norm(d), axis, atol=1e-6)
            az, el = azimuth_elevation(face_dirs(face, 4))
            self.assertTrue(np.isfinite(az).all() and np.isfinite(el).all())

    def test_a_star_lands_on_the_texel_whose_direction_it_has(self):
        n = 32
        for face in range(6):
            d = face_dirs(face, n)
            for r, c in ((5, 7), (16, 30), (31, 0)):
                img = np.zeros((n, n, 3), np.float32)
                stars = (
                    d[r, c][None].astype(np.float32),
                    np.array([0.3], np.float32),
                    np.ones((1, 3), np.float32),
                )
                draw_stars(img, face, n, stars)
                peak = np.unravel_index(img[..., 0].argmax(), (n, n))
                self.assertEqual(peak, (r, c), f"face {face}")

    def test_bake_keeps_the_art_where_the_sphere_shows_it(self):
        # a red band across the middle of black art: red at the horizon, dark straight up
        tex = np.zeros((64, 64, 3), np.uint8)
        tex[24:40, :, 0] = 200
        pos, uv = uv_sphere(2, 0.5, -0.2, 0.5)
        img = bake(
            tex, pos, uv, 16, seed=3, stars=0
        )  # at 16 texels a face, stars would cover it
        self.assertEqual(img.shape, (96, 16, 3))
        np.testing.assert_array_equal(img, bake(tex, pos, uv, 16, seed=3, stars=0))
        side = img[:16, :, :].astype(float)  # +X face, its middle rows at the horizon
        up = img[32:48, :, :].astype(float)
        self.assertGreater(side[6:10, :, 0].mean(), 3 * up[6:10, 6:10, 0].mean() + 20)
        self.assertGreater(side[6:10, :, 0].mean(), 2 * side[6:10, :, 2].mean())

    def test_split_stars_leave_the_nebula_clean_and_keep_the_light(self):
        tex = np.full((64, 64, 3), 90, np.uint8)
        pos, uv = uv_sphere(2, 0.5, -0.2, 0.5)
        whole, none = bake_layers(tex, pos, uv, 32, seed=3, stars=400)
        self.assertIsNone(none)
        clean, stars = bake_layers(
            tex, pos, uv, 32, seed=3, stars=400, split_stars=True
        )
        self.assertEqual(
            (clean.shape, stars.shape, stars.dtype),
            ((192, 32, 3), (192, 32, 3), np.uint16),
        )
        self.assertGreater(stars.max(), 0)
        self.assertLessEqual(clean.astype(int).sum(), whole.astype(int).sum())
        self.assertTrue((clean != whole).any(), "the stars are not in the nebula cube")

    def test_sharing_a_face_between_skies_gives_the_one_sky_pixels(self):
        pos, uv = uv_sphere(2, 0.5, -0.2, 0.5)
        skies = [np.full((64, 64, 3), g, np.uint8) for g in (40, 120)]
        skies[1][20:44, :, 2] = 220
        size, seed = 24, 5
        want = [bake_layers(t, pos, uv, size, seed, 300, True) for t in skies]
        guides = [SkyGuide(t, pos, uv) for t in skies]
        got = [([], []) for _ in skies]
        for face in range(6):
            for band in range(BANDS):
                base = FaceBase(
                    face, size, seed, 300, band
                )  # computed once, used by both skies
                for k, guide in enumerate(guides):
                    neb, stars = shade_face(base, guide, True)
                    got[k][0].append(neb)
                    got[k][1].append(stars)
        for k in range(len(skies)):
            np.testing.assert_array_equal(np.concatenate(got[k][0]), want[k][0])
            np.testing.assert_array_equal(np.concatenate(got[k][1]), want[k][1])

    def test_16_bit_png_round_trips(self):
        img = np.arange(4 * 3 * 3, dtype=np.uint16).reshape(4, 3, 3) * 5000
        img[0, 0] = (65535, 0, 1)
        with tempfile.TemporaryDirectory() as d:
            path = Path(d, "s.png")
            write_png16(path, img)
            np.testing.assert_array_equal(read_png16(path), img)
        self.assertGreater(STAR_GAIN, 1.0)


if __name__ == "__main__":
    unittest.main()
