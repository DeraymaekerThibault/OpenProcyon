"""Waypoint art from BD_Textures (DDS): HUD_Waypointline2 (course dash, additive, black = transparent) and the HUD_Waypoint_* stars
(DXT1 on white: white becomes transparent, edge pixels are un-mixed from the white so no halo remains).
usage: python3 tools/export_waypoint_art.py <game root> <assets dir>"""

import glob
import os
import sys

import numpy as np
from PIL import Image

root, out = sys.argv[1:3]
for f in glob.glob(root + "/BD_Textures/HUD_Waypoint*.dds"):
    name = os.path.basename(f)[:-4].lower()
    im = Image.open(f).convert("RGBA")
    if name != "hud_waypointline2":
        a = np.array(im).astype(np.float32)
        alpha = 255.0 - a[..., :3].min(axis=2)
        k = np.maximum(alpha, 1.0)[..., None] / 255.0
        a[..., :3] = np.clip((a[..., :3] - (1.0 - k) * 255.0) / k, 0, 255)
        a[..., 3] = alpha
        im = Image.fromarray(a.astype(np.uint8), "RGBA")
    im.save(f"{out}/hud/tex/{name}.png")
