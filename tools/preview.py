"""Tiny painter's-algorithm preview of MDB layers (validation aid, not a renderer)."""

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw

from tools.mdb import parse


def load_tex(root, name):
    p = Path(root) / "BD_Textures" / (name.rsplit(".", 1)[0] + ".dds")
    try:
        return Image.open(p).convert("RGBA")
    except OSError:
        return None


def render(root, layers, model, yaw, pitch, size=700, points=()):
    tris, texcache = [], {}
    for path in layers:
        mdb = parse(Path(path).read_bytes())
        m = mdb.models[model]
        for a, b, c, mat in m.faces:
            tex = texcache.setdefault(
                mat,
                load_tex(root, mdb.textures[mat]) if mat < len(mdb.textures) else None,
            )
            tris.append(([m.vertices[i] for i in (a, b, c)], tex))
    cy, sy, cp, sp = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch)

    def proj(
        p,
    ):  # z-up world; rotate about z (yaw) then tilt (pitch); depth = distance from camera
        x, y, z = p
        x, y = x * cy - y * sy, x * sy + y * cy
        return x, z * cp - y * sp, y * cp + z * sp

    pts = [proj(v.pos) for t, _ in tris for v in t]
    marks = [(proj(p), c) for p, c in points]
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    s = size * 0.9 / max(max(xs) - min(xs), max(ys) - min(ys))
    ox, oy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    img = Image.new("RGB", (size, size), (20, 24, 32))
    d = ImageDraw.Draw(img)
    drawn = []
    for t, tex in tris:
        q = [proj(v.pos) for v in t]
        col = (160, 160, 160)
        if tex:
            u = sum(v.uv[0] for v in t) / 3 % 1
            w = sum(v.uv[1] for v in t) / 3 % 1
            col = tex.getpixel((int(u * (tex.width - 1)), int(w * (tex.height - 1))))[
                :3
            ]
        drawn.append(
            (
                sum(p[2] for p in q) / 3,
                [(size / 2 + (p[0] - ox) * s, size / 2 - (p[1] - oy) * s) for p in q],
                col,
            )
        )
    for _, poly, col in sorted(drawn, key=lambda t: -t[0]):
        d.polygon(poly, fill=col, outline=tuple(c // 2 for c in col))
    for (x, y, _), c in marks:
        px, py = size / 2 + (x - ox) * s, size / 2 - (y - oy) * s
        d.ellipse((px - 5, py - 5, px + 5, py + 5), fill=c, outline=(0, 0, 0))
    return img


if __name__ == "__main__":
    from tools.mdb_to_glb import hardpoints

    root, out, scene, *layers = sys.argv[1:]
    colors = {
        "GunPlacement": (255, 40, 40),
        "EnginePortPlacement": (40, 255, 255),
        "FlagAttachmentPoint": (255, 255, 255),
    }
    points = [
        (p, colors.get(n.kind, (60, 255, 60))) for n, p in hardpoints(root, scene)
    ]
    views = [(0, 0), (math.pi / 2, 0), (0.6, 0.5), (0, math.pi / 2)]
    sheet = Image.new("RGB", (1400, 1400))
    for i, (yaw, pitch) in enumerate(views):
        sheet.paste(
            render(root, layers, 0, yaw, pitch, points=points),
            ((i % 2) * 700, (i // 2) * 700),
        )
    sheet.save(out)
