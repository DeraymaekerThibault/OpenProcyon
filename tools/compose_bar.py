"""Composes the static main bar of each faction skin into one image, so the game draws it as one node (no seams between parts at
fractional UI scales). Reads the part list `content/data/hud/bar.json` and the exported sprites (tools/hud_sprites.py).

    hud/bar/<skin>.png    the idle art of every part (and the stretched backdrops behind them), butted exactly, 1x reference pixels
    hud/bar/<skin>.json   {"width": W, "height": H, "parts": [{"sprite", "x", "width", "height"}, ..]} in reference pixels

Layout (the same the game used to do with flex rows): a part is as wide as its sprite, less its transparent right columns when the
skin has no stretched backdrops (Pirate, Procyon); parts of the Navy set share one edge pixel. A run of parts with the same backdrop
sits on a tiled strip of it. Parts share their top edge and the image is as tall as the shortest part (taller parts run off-screen).
The game draws the dynamic pieces (button hover/press art, gauge fills) on top at the part boxes.
A mod can ship its own `hud/bar/<skin>.png` + `.json`; the game reads only those two files.
usage: python3 tools/compose_bar.py <assets dir> [bar.json]"""

import json
import sys
from pathlib import Path

from PIL import Image

from tools.hud_sprites import IDLE, SKINS, read, sprite_dir

BAR = Path(__file__).resolve().parent.parent / "content/data/hud/bar.json"


def info(out, name, skin):
    """(untrimmed idle image, [w, h], [left, top, right, bottom]) of a sprite, or None when the skin lacks it."""
    meta = sprite_dir(out, name) / f"{skin}.json"
    image = read(out, name, skin)
    if image is None:
        return None
    state = json.loads(meta.read_text())["states"][IDLE]
    return image, state["size"], state["trim"]


def white_edge(image, width):
    """True when the last of the first `width` columns is a white line (the Procyon caps end on one): art the neighbour should
    not butt against, so the column is left out of the layout."""
    column = [image.getpixel((width - 1, y)) for y in range(image.height)]
    seen = [p for p in column if p[3] > 200]
    return len(seen) > 0 and sum(min(p[:3]) >= 250 for p in seen) >= 0.8 * len(seen)


def layout(parts, wings):
    """Groups `parts` (dicts with width, backdrop) into runs and places them: returns (runs, total width). A run is
    (backdrop name or None, x, width, [(x, part), ..]); a part without a backdrop is a run of its own. Runs and parts overlap
    their neighbour by one pixel in a skin with stretched backdrops."""
    overlap = 1 if wings else 0
    runs = []
    for p in parts:
        if p["backdrop"] and runs and runs[-1][0] == p["backdrop"]:
            runs[-1][1].append(p)
        else:
            runs.append((p["backdrop"], [p]))
    placed, x = [], 0
    for strip, members in runs:
        start, inside = x, []
        for p in members:
            inside.append((x, p))
            x += p["width"] - overlap
        placed.append((strip, start, x + overlap - start, inside))
    total = placed[-1][1] + placed[-1][2]
    return placed, total


def compose_skin(out, bar, skin):
    wings = info(out, "MainBar_LeftStretchedDetail", skin) is not None
    parts = []
    for entry in bar:
        got = info(out, entry["sprite"], skin)
        if got is None:
            continue
        image, size, trim = got
        width = size[0] if wings else size[0] - trim[2]
        if white_edge(image, width):
            width -= 1
        if width <= 0:
            continue
        strip = entry.get("backdrop")
        tile = info(out, strip, skin) if strip else None
        parts.append(
            dict(
                sprite=entry["sprite"],
                image=image,
                width=width,
                height=size[1],
                backdrop=strip if tile and tile[1][0] > 1 else None,
                tile=tile,
            )
        )
    if not parts:
        return None
    height = min(p["height"] for p in parts)
    runs, total = layout(parts, wings)
    canvas = Image.new("RGBA", (total, height))

    def paste(image, x, limit):
        piece = Image.new("RGBA", canvas.size)
        piece.paste(
            image.crop((0, 0, min(limit, image.width), min(height, image.height))),
            (x, 0),
        )
        canvas.alpha_composite(piece)

    for strip, start, width, inside in runs:
        if strip:
            tile = inside[0][1]["tile"]
            step = tile[1][0] - 1
            for k in range(-(-width // step)):
                paste(tile[0], start + k * step, width - k * step)
        for x, p in inside:
            paste(p["image"], x, p["width"])
    placed = [(x, p) for _, _, _, inside in runs for x, p in inside]
    data = dict(
        width=total,
        height=height,
        parts=[
            dict(
                sprite=p["sprite"],
                x=x,
                width=p["width"],
                height=min(p["height"], height),
            )
            for x, p in placed
        ],
    )
    return canvas, data


def main(out, bar_file=BAR):
    out = Path(out)
    bar = json.loads(Path(bar_file).read_text())
    (out / "hud/bar").mkdir(parents=True, exist_ok=True)
    for skin in SKINS:
        done = compose_skin(out, bar, skin)
        if done is None:
            continue
        canvas, data = done
        canvas.save(out / f"hud/bar/{skin.lower()}.png")
        (out / f"hud/bar/{skin.lower()}.json").write_text(json.dumps(data, indent=1))


if __name__ == "__main__":
    main(*sys.argv[1:])
