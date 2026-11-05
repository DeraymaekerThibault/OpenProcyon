"""Joins retail strips (left cap | stretchy centre | right cap, or just two pieces) into single images so the game can draw them as
Bevy 9-slice nodes (seam-free at any scale). The groups are data: content/data/hud/slices.json,

    {"<group>": {"left": component, "center": component, "right": component, "states": [..], "caps": [left, right]}}

`center` is optional (two pieces are butted, each less its fully transparent inner column); `states` defaults to ["NoCursor"];
`caps` (reference px of each end that must not stretch) defaults to the widths of `left` and `right`, which the game reads from
the same file. Output: <out>/hud/tex/slice_<group>_<skin>_<state>.png, 1x reference pixels, for every skin that has all the pieces.
The pieces are read from hud/sprites (untrimmed again, see tools/hud_sprites.py).
HD replacement: ship a 2x PNG of the same name in a mod plus hud/scales.json {"slice_...": 2}.
usage: python3 tools/make_slices.py <assets dir> [slices.json [out dir]]"""

import json
import sys
from pathlib import Path

from PIL import Image

from tools.hud_sprites import SKINS, read

SLICES = Path(__file__).resolve().parent.parent / "content/data/hud/slices.json"


def inner_trim(image, left_edge, right_edge):
    """`image` less its fully transparent columns on the chosen edges."""
    box = image.getchannel("A").getbbox()
    if box is None:
        return image
    x0 = box[0] if left_edge else 0
    x1 = box[2] if right_edge else image.width
    return image.crop((x0, 0, x1, image.height))


def join(parts, butted):
    """The pieces side by side, top-aligned; `butted` (no centre) also drops transparent columns where the pieces meet."""
    if butted:
        parts = [inner_trim(parts[0], False, True), inner_trim(parts[1], True, False)]
    image = Image.new(
        "RGBA", (sum(p.width for p in parts), max(p.height for p in parts))
    )
    x = 0
    for p in parts:
        image.paste(p, (x, 0))
        x += p.width
    return image


def main(out, slices=SLICES, tex=None):
    out = Path(out)
    tex = Path(tex) if tex else out / "hud/tex"
    tex.mkdir(parents=True, exist_ok=True)
    for group, spec in json.loads(Path(slices).read_text()).items():
        names = [spec[k] for k in ("left", "center", "right") if k in spec]
        for skin in SKINS:
            for state in spec.get("states", ["NoCursor"]):
                parts = [read(out, n, skin, state) for n in names]
                if any(p is None for p in parts):
                    continue
                join(parts, "center" not in spec).save(
                    tex / f"slice_{group}_{skin.lower()}_{state.lower()}.png"
                )


if __name__ == "__main__":
    main(*sys.argv[1:])
