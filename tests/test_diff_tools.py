"""tools/tracediff.py and tools/visdiff.py comparisons on small synthetic inputs (no game is launched)."""

import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

from tools import tracediff, visdiff


class TraceDiffTests(unittest.TestCase):
    def test_same_and_first_difference(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, c = (Path(d, n) for n in ("a.csv", "b.csv", "c.csv"))
            rows = "gt,ship,x\n0.033,0,1.0\n0.066,0,2.0\n"
            a.write_text(rows)
            b.write_text(rows)
            c.write_text(rows.replace("2.0", "2.5") + "0.099,0,3.0\n")
            self.assertEqual(tracediff.main([str(a), str(b)]), 0)
            line, first = tracediff.compare(a, c)
            self.assertEqual(
                line, "DIFFERENT a.csv: 2 of 3 rows, first at line 3 gt=0.066"
            )
            self.assertEqual(first, ("0.066,0,2.0\n", "0.066,0,2.5\n"))
            self.assertEqual(tracediff.main([str(a), str(c)]), 1)


class VisDiffTests(unittest.TestCase):
    def test_identical_pictures_are_same_and_a_changed_block_is_boxed(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, diff = Path(d, "a.png"), Path(d, "b.png"), Path(d, "diff.png")
            px = np.zeros((20, 30, 3), np.uint8)
            Image.fromarray(px).save(a)
            Image.fromarray(px).save(b)
            self.assertEqual(visdiff.compare(a, b, diff), (0.0, None))
            self.assertFalse(diff.exists())
            px[5:8, 10:14] = 200
            Image.fromarray(px).save(b)
            frac, box = visdiff.compare(a, b, diff)
            self.assertAlmostEqual(frac, 12 / 600)
            self.assertEqual(tuple(int(v) for v in box), (10, 5, 13, 7))
            self.assertTrue(diff.exists())


if __name__ == "__main__":
    unittest.main()
