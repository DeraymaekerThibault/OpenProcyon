"""Parser for Fonts/*.fft bitmap fonts -> {height, textures, chars: {ch: {row, x, w, tex, next}}}.

Glyph pixel rect on the texture: x = Start Pixel On Row, y = Row Number * Height In Pixels, size = (Width, Height).
"""

import json
import re
import sys


def parse(text):
    font = {"height": 0, "textures": [], "chars": {}}
    cur = None
    for raw in text.replace("\r", "").split("\n"):
        s = raw.strip()
        m = re.match(r"^[0-9a-f]{8} Character: '(.*)'$", s)
        if m:
            cur = font["chars"].setdefault(m.group(1), {})
            continue
        m = re.match(r"^Texture Names - Element String '(.*)'$", s)
        if m:
            font["textures"].append(m.group(1))
            continue
        m = re.match(r"^Height In Pixels Int (\d+)$", s)
        if m:
            font["height"] = int(m.group(1))
            continue
        if cur is None:
            continue
        for key, name in (
            ("Row Number", "row"),
            ("Start Pixel On Row", "x"),
            ("Width In Pixels", "w"),
            ("Texture Index", "tex"),
            ("Next Character Start Pixel Offset", "next"),
        ):
            m = re.match(rf"^{key} Int (-?\d+)$", s)
            if m:
                cur[name] = int(m.group(1))
    return font


if __name__ == "__main__":
    with open(sys.argv[1], encoding="latin1") as f:
        print(json.dumps(parse(f.read())))
