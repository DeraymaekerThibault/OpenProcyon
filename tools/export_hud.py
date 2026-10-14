"""Export the in-game HUD: assets/hud/hud.json (components) + assets/hud/tex/<sheet>.png.

usage: python -m tools.export_hud <game_root> <outdir>
"""

import json
import sys
from pathlib import Path
from PIL import Image
from tools.hdt import parse
from tools.fft import parse as parse_font
from tools.export_effects import ci_files


def main(root, out):
    comps = parse(Path(ci_files(Path(root), "hud.hdt")[0]).read_text(encoding="latin1"))
    out = Path(out) / "hud"
    (out / "tex").mkdir(parents=True, exist_ok=True)
    # Every cursor state's sheet (the radar's hover/pressed art is on another sheet than its idle art).
    sheets = {
        v.lower()
        for c in comps.values()
        for d in c.values()
        if isinstance(d, dict)
        for k, v in d.items()
        if k.startswith("TextureName - ") and v
    }
    missing = []
    for s in sorted(sheets):
        dst = out / "tex" / f"{s}.png"
        src = ci_files(Path(root) / "BD_GUITextures", s + ".tga")
        if not src:
            missing.append(s)
        elif not dst.exists():
            Image.open(src[0]).convert("RGBA").save(dst)
    (out / "hud.json").write_text(json.dumps(comps))
    (out / "fonts").mkdir(exist_ok=True)
    for f in ci_files(Path(root) / "Fonts", "*.fft"):
        font = parse_font(Path(f).read_text(encoding="latin1"))
        (out / "fonts" / (Path(f).stem + ".json")).write_text(json.dumps(font))
        for t in font["textures"]:
            dst = out / "tex" / f"{t.lower()}.png"
            src = ci_files(Path(root) / "BD_GUITextures", t + ".tga")
            if src and not dst.exists():
                Image.open(src[0]).convert("RGBA").save(dst)
    print(
        f"exported {len(comps)} HUD components, {len(sheets) - len(missing)} sheets; missing: {missing}"
    )


if __name__ == "__main__":
    main(*sys.argv[1:3])
