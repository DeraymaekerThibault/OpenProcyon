"""Export the in-game HUD: hud/hud.json (components without their art), one trimmed PNG per sprite under hud/sprites/ (layout in
tools/hud_sprites.py) and the font metrics in hud/fonts/ (no glyph sheets: UI text is vector).

usage: python -m tools.export_hud <game_root> <outdir>
"""

import json
import re
import shutil
import sys
from pathlib import Path

from PIL import Image

from tools.fft import parse as parse_font
from tools.fix_alpha_bleed import bleed
from tools.gamefs import ci_files
from tools.hdt import parse
from tools.hud_sprites import IDLE, SKINS, sprite_dir, trim_image

SAFE = re.compile(r"^[A-Za-z0-9_ ]+$")
ART = ("TextureName - ", "TextureArea - ", "TextureAreaTrimmed - ")


def art_of(skin):
    """{state: (sheet, (x0, y0, x1, y1))} of one skin of a component: the states that have a sheet and a rectangle."""
    states = {}
    for key, sheet in skin.items():
        if not key.startswith("TextureName - ") or not sheet:
            continue
        state = key[len("TextureName - ") :]
        area = skin.get(f"TextureArea - {state}")
        if isinstance(area, str):  # one retail entry lacks its Rect( ) wrapper
            area = [int(n) for n in re.findall(r"-?\d+", area)]
        if area and len(area) == 4:
            states[state] = (sheet.lower(), tuple(area))
    return states


def distinct(states):
    """The states that need their own file: NoCursor, and every other state whose art differs from it (the rest draw as NoCursor)."""
    return {
        s: art
        for s, art in states.items()
        if s == IDLE or IDLE not in states or art != states[IDLE]
    }


def main(root, out):
    comps = parse(Path(ci_files(Path(root), "hud.hdt")[0]).read_text(encoding="latin1"))
    out = Path(out)
    hud = out / "hud"
    (hud / "tex").mkdir(parents=True, exist_ok=True)
    # Sprites are rewritten from the retail art every time, so a reimport into a filled folder equals a fresh one.
    shutil.rmtree(hud / "sprites", ignore_errors=True)
    # Atlases and bar crops written by earlier versions are not used any more.
    old = {
        n
        for c in comps.values()
        for d in c.values()
        if isinstance(d, dict)
        for k, n in d.items()
        if k.startswith("TextureName - ") and n
    }
    for f in [hud / "tex" / f"{n.lower()}.png" for n in old] + list(
        (hud / "tex").glob("bar_*.png")
    ):
        f.unlink(missing_ok=True)
    sheets, missing, count = {}, set(), 0

    def sheet(name):
        if name not in sheets:
            src = ci_files(Path(root) / "BD_GUITextures", name + ".tga")
            sheets[name] = Image.open(src[0]).convert("RGBA") if src else None
            if not src:
                missing.add(name)
        return sheets[name]

    for name, comp in comps.items():
        for skin_name in SKINS:
            skin = comp.get(skin_name)
            if not skin or not SAFE.match(name):
                continue
            meta = {k: v for k, v in skin.items() if not k.startswith(ART)}
            meta["states"] = {}
            for state, (sheet_name, (x0, y0, x1, y1)) in distinct(art_of(skin)).items():
                image = sheet(sheet_name)
                if image is None or x1 < x0 or y1 < y0:
                    continue
                # A few retail rectangles reach past their sheet; the part outside is transparent.
                cell = image.crop((x0, y0, x1 + 1, y1 + 1))
                trimmed, trim = trim_image(cell)
                fixed = bleed(trimmed)
                trimmed = trimmed if fixed is None else fixed
                path = sprite_dir(out, name) / skin_name / f"{state}.png"
                path.parent.mkdir(parents=True, exist_ok=True)
                trimmed.save(path)
                meta["states"][state] = {"size": list(cell.size), "trim": trim}
                count += 1
            if meta["states"]:
                (sprite_dir(out, name) / f"{skin_name}.json").write_text(
                    json.dumps(meta)
                )
    # hud.json keeps the layout numbers, timings and offsets; the art lives in hud/sprites.
    data = {
        name: {
            s: (
                {k: v for k, v in d.items() if not k.startswith(ART)}
                if isinstance(d, dict)
                else d
            )
            for s, d in comp.items()
        }
        for name, comp in comps.items()
    }
    (hud / "hud.json").write_text(json.dumps(data))
    (hud / "fonts").mkdir(exist_ok=True)
    for f in ci_files(Path(root) / "Fonts", "*.fft"):
        font = parse_font(Path(f).read_text(encoding="latin1"))
        (hud / "fonts" / (Path(f).stem + ".json")).write_text(json.dumps(font))
    print(
        f"exported {len(comps)} HUD components, {count} sprites; missing sheets: {sorted(missing)}"
    )


if __name__ == "__main__":
    main(*sys.argv[1:3])
