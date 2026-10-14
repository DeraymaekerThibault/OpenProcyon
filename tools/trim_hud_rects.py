"""Trim HUD bar rectangles and export isolated bar sprites to prevent atlas bleed.
The original rectangles remain available for layout and other widgets.
Usage: python3 tools/trim_hud_rects.py <assets dir>
"""

import json
import sys
from pathlib import Path

from PIL import Image

# Hand-written HUD layout that ships with the code (content/), not imported.
BAR = Path(__file__).resolve().parents[1] / "content/data/hud/bar.json"


def trimmed_rect(alpha, area):
    x0, y0, x1, y1 = area
    # A few retail rectangles extend outside the sheet; do not reinterpret those.
    if not (0 <= x0 <= x1 < alpha.width and 0 <= y0 <= y1 < alpha.height):
        return list(area)
    while x1 > x0 and alpha.crop((x1, y0, x1 + 1, y1 + 1)).getbbox() is None:
        x1 -= 1
    return [x0, y0, x1, y1]


def main(assets, bar=BAR):
    assets = Path(assets)
    path = assets / "hud/hud.json"
    hud = json.loads(path.read_text())
    sheets = {}
    bar_names = {
        part["sprite"]
        for part in json.loads(Path(bar).read_text())
    }
    for name, comp in hud.items():
        if not isinstance(comp, dict):
            continue
        for faction, skin in comp.items():
            if not isinstance(skin, dict):
                continue
            skin.pop("TextureAreaTrimmed - NoCursor", None)
            if not name.startswith(("MainBar_", "MAINBAR_")):
                continue
            area, sheet = (
                skin.get("TextureArea - NoCursor"),
                skin.get("TextureName - NoCursor"),
            )
            if not area or not sheet:
                continue
            if sheet not in sheets:
                f = assets / "hud/tex" / f"{sheet.lower()}.png"
                sheets[sheet] = (
                    Image.open(f).convert("RGBA").getchannel("A")
                    if f.exists()
                    else None
                )
            alpha = sheets[sheet]
            if alpha is not None:
                rect = trimmed_rect(alpha, area)
                skin["TextureAreaTrimmed - NoCursor"] = rect
                if name in bar_names:
                    x0, y0, x1, y1 = rect if faction != "Navy" else area
                    with Image.open(
                        assets / "hud/tex" / f"{sheet.lower()}.png"
                    ) as image:
                        image.crop((x0, y0, x1 + 1, y1 + 1)).save(
                            assets
                            / "hud/tex"
                            / f"bar_{faction.lower()}_{name.lower()}.png"
                        )
    path.write_text(json.dumps(hud))


if __name__ == "__main__":
    main(sys.argv[1])
