"""Joins the retail 3-piece strips (left cap | stretchy centre | right cap) into single images so the game can draw them as
Bevy 9-slice nodes (seam-free at any scale). Output: assets/hud/tex/slice_<group>_<skin>_<state>.png, 1x reference pixels.
The pieces are read from hud/sprites (untrimmed again, see tools/hud_sprites.py).
HD replacement: ship a 2x PNG of the same name in a mod plus hud/scales.json {"slice_...": 2}.
usage: python3 tools/make_slices.py <assets dir>"""

import sys

from PIL import Image

from tools.hud_sprites import SKINS, read

GROUPS = {  # group: (left, centre, right component names)
    "button": [f"FrontEnd_Texts_Over_Button_{p}" for p in ("Left", "Center", "Right")],
    "ribbon": [
        f"ShipNameBackdrop_{p}_WhiteBanner" for p in ("Left", "Center", "Right")
    ],
}


def main(out):
    for group, names in GROUPS.items():
        for skin in SKINS:
            for state in (
                ["NoCursor", "CursorOver", "CursorDown"]
                if group == "button"
                else ["NoCursor"]
            ):
                parts = [read(out, n, skin, state) for n in names]
                if any(p is None for p in parts):
                    continue
                im = Image.new("RGBA", (sum(p.width for p in parts), parts[0].height))
                x = 0
                for p in parts:
                    im.paste(p, (x, 0))
                    x += p.width
                im.save(
                    f"{out}/hud/tex/slice_{group}_{skin.lower()}_{state.lower()}.png"
                )


if __name__ == "__main__":
    from pathlib import Path

    main(Path(sys.argv[1]))
