"""Waypoint art from BD_Textures (DDS): HUD_Waypointline2 (course dash, additive, black = transparent) and the HUD_Waypoint_* stars
(DXT1 on white: white becomes transparent, edge pixels are un-mixed from the white so no halo remains).
usage: python3 tools/export_waypoint_art.py <game root> <assets dir>"""

from pathlib import Path

import numpy as np
from PIL import Image

from tools import cli
from tools.gamefs import ci_files


def export(root, out):
    for f in ci_files(Path(root) / "BD_Textures", "HUD_Waypoint*.dds"):
        name = f.stem.lower()
        im = Image.open(f).convert("RGBA")
        if name != "hud_waypointline2":
            a = np.array(im).astype(np.float32)
            alpha = 255.0 - a[..., :3].min(axis=2)
            k = np.maximum(alpha, 1.0)[..., None] / 255.0
            a[..., :3] = np.clip((a[..., :3] - (1.0 - k) * 255.0) / k, 0, 255)
            a[..., 3] = alpha
            im = Image.fromarray(a.astype(np.uint8), "RGBA")
        im.save(f"{out}/hud/tex/{name}.png")


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
