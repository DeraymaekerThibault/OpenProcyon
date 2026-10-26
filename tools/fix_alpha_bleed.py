"""Give fully transparent pixels the colour of their nearest opaque neighbours.

The retail sheets keep white (or black) RGB under alpha 0; bilinear sampling at non-integer UI scales then
pulls that colour into sprite edges (light hairlines, dark fringes). Run on UI sheets after export; idempotent.
usage: python3 tools/fix_alpha_bleed.py assets/hud/tex assets/menu ..."""

import pathlib
import sys

import numpy as np
from PIL import Image


def fix(path):
    im = Image.open(path)
    if im.mode != "RGBA":
        return False
    a = np.array(im)
    alpha = a[..., 3]
    known = alpha > 0
    if known.all() or not known.any():
        return False
    rgb = a[..., :3].copy()
    todo = ~known
    for _ in range(4):  # a few texels is enough for bilinear/mip bleeding
        new = todo.copy()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            src_k = np.roll(known, (dy, dx), (0, 1))
            src_c = np.roll(rgb, (dy, dx), (0, 1))
            take = todo & src_k
            take &= new
            rgb[take] = src_c[take]
            new &= ~take
            known = known | take
        todo = ~known
    a[..., :3] = rgb
    Image.fromarray(a, "RGBA").save(path, optimize=True)
    return True


if __name__ == "__main__":
    n = 0
    for d in sys.argv[1:]:
        for p in sorted(pathlib.Path(d).glob("*.png")):
            n += fix(p)
    print("fixed", n)
