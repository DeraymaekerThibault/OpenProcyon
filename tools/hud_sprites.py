"""The per-sprite HUD art layout written by tools/export_hud.py and read by the game, the slice builder and the asset comparison.

    hud/sprites/<component>/<skin>.json            reference sizes, trims and offsets of one skin of a hud.hdt component
    hud/sprites/<component>/<skin>/<state>.png     the art of one cursor state, without its fully transparent border

`<skin>.json` is `{"TopLeft X": .., "TopLeft Y": .., "states": {"NoCursor": {"size": [w, h], "trim": [l, t, r, b]}, ..}}`:
`size` is the sprite's untrimmed reference size (the rectangle in hud.hdt) and `trim` the reference pixels cut off the left, top,
right and bottom edge. A state that the data draws exactly like NoCursor has no entry and no file.
"""

import json
from pathlib import Path

from PIL import Image

SKINS = ("Navy", "Pirate", "Procyon")
IDLE = "NoCursor"


def sprite_dir(out, component):
    return Path(out) / "hud/sprites" / component


def trim_image(image):
    """(trimmed image, [left, top, right, bottom]) with fully transparent rows and columns cut from all four sides. A sprite
    with no visible pixel keeps one transparent pixel, so the file still exists and the game draws nothing."""
    image = image.convert("RGBA")
    box = image.getchannel("A").getbbox()
    w, h = image.size
    if box is None:
        return Image.new("RGBA", (1, 1)), [0, 0, w - 1, h - 1]
    return image.crop(box), [box[0], box[1], w - box[2], h - box[3]]


def untrimmed(image, size, trim):
    """The sprite on its full reference canvas again (`image` may be a higher-resolution copy; the canvas scales with it)."""
    (w, h), (left, top, right, _) = size, trim
    k = image.width / max(w - left - right, 1)
    canvas = Image.new("RGBA", (round(w * k), round(h * k)))
    canvas.paste(image.convert("RGBA"), (round(left * k), round(top * k)))
    return canvas


def read(out, component, skin, state=IDLE):
    """The untrimmed sprite as exported under `out`, or None when that state has no art (a missing state draws as NoCursor)."""
    meta = sprite_dir(out, component) / f"{skin}.json"
    if not meta.is_file():
        return None
    states = json.loads(meta.read_text())["states"]
    info = states.get(state) or (states.get(IDLE) if state != IDLE else None)
    if info is None:
        return None
    path = (
        sprite_dir(out, component) / skin / f"{state if state in states else IDLE}.png"
    )
    if not path.is_file():
        return None
    with Image.open(path) as image:
        return untrimmed(image, info["size"], info["trim"])
