"""Joins the retail 3-piece strips (left cap | stretchy centre | right cap) into single images so the game can draw them as
Bevy 9-slice nodes (seam-free at any scale). Output: assets/hud/tex/slice_<group>_<skin>_<state>.png, 1x reference pixels.
HD replacement: ship a 2x PNG of the same name in a mod plus hud/scales.json {"slice_...": 2}.
usage: python3 tools/make_slices.py <assets dir>"""

import json
import sys

from PIL import Image

out = sys.argv[1]
with open(f"{out}/hud/hud.json") as f:
    hud = json.load(f)
GROUPS = {  # group: (left, centre, right component names)
    "button": [f"FrontEnd_Texts_Over_Button_{p}" for p in ("Left", "Center", "Right")],
    "ribbon": [
        f"ShipNameBackdrop_{p}_WhiteBanner" for p in ("Left", "Center", "Right")
    ],
}
for group, names in GROUPS.items():
    for skin in hud[names[0]]:
        for state in (
            ["NoCursor", "CursorOver", "CursorDown"]
            if group == "button"
            else ["NoCursor"]
        ):
            try:
                parts = []
                for n in names:
                    d = hud[n][skin]
                    x0, y0, x1, y1 = d[f"TextureArea - {state}"]
                    sheet = Image.open(
                        f"{out}/hud/tex/{d[f'TextureName - {state}'].lower()}.png"
                    ).convert("RGBA")
                    parts.append(sheet.crop((x0, y0, x1 + 1, y1 + 1)))
            except (KeyError, FileNotFoundError):
                continue
            im = Image.new("RGBA", (sum(p.width for p in parts), parts[0].height))
            x = 0
            for p in parts:
                im.paste(p, (x, 0))
                x += p.width
            im.save(f"{out}/hud/tex/slice_{group}_{skin.lower()}_{state.lower()}.png")
