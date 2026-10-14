"""HUD texture overlays named by 'Open HUD Texture Overlay' rules (Credit*, TUT*, TUTSHORT*): the retail GUI textures are split in
256x256 tiles `<Name>_<x>_<y>.tga`; they are stitched into assets/hud/overlay/<name>.png (lowercase).
usage: python -m tools.export_overlays <game_root> <assets_dir>"""

import json
import re
import sys
from pathlib import Path

from PIL import Image


def main(root, out):
    root, out = Path(root), Path(out)
    names = set()
    # Missions sit in campaign/ during an import and in maps/<id>/ after pack_layout.
    missions = [*(out / "maps").glob("*/mission.json"), *(f for f in (out / "campaign").glob("*.json") if not f.stem.endswith("_media"))]
    for f in missions:
        for r in json.loads(f.read_text()).get("rules", []):
            names |= {
                a["Texture Base Name"]
                for a in r["actions"]
                if a["Type"] == "Open HUD Texture Overlay"
            }
    dest = out / "hud/overlay"
    dest.mkdir(parents=True, exist_ok=True)
    tex = {p.name.lower(): p for p in (root / "BD_GUITextures").glob("*.tga")}
    for n in sorted(names):
        tiles = {}
        for k, p in tex.items():
            m = re.fullmatch(re.escape(n.lower()) + r"_(\d+)_(\d+)\.tga", k)
            if m:
                tiles[(int(m[1]), int(m[2]))] = Image.open(p).convert("RGBA")
        if not tiles:
            print("missing", n)
            continue
        w = max(x for x, _ in tiles) + 1
        h = max(y for _, y in tiles) + 1
        img = Image.new("RGBA", (256 * w, 256 * h))
        for (x, y), t in tiles.items():
            img.alpha_composite(t, (256 * x, 256 * y))
        img.save(dest / f"{n.lower()}.png")
    print(len(names), "overlays")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
