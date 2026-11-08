"""Bake surface maps from the player's imported model textures (not retail: an optional visual upgrade for the render
profile's `surfaces` block).

The retail models carry only colour textures. Without a map, `shaders/surface.wesl` reads relief from the texture's
brightness as it draws, one texel around each pixel. This bakes the same quantities once, with filtering a shader cannot
afford per pixel: the compression noise smoothed away and the relief taken at several scales, so rock, asteroid and
island textures get their larger lumps and hollows as well as the fine grain. One RGBA PNG per retail material,
`textures/surface/<material name without .tga, lowercase>.png`, at the texture's own size, linear:
R, G = height slope along u and v (per texel, times SLOPE_GAIN, 0.5 = flat), B = crevice depth (0..1), A = height (the
texture's luma). Roughness and metal stay per material in the profile's rules (look.json).
The result is derived from the player's own copy, so it stays local: by default a mod folder `<user dir>/mods/baked-surfaces`,
switched on in the Mods screen. Needs only NumPy and Pillow.
usage: python3 -m tools.bake_surfaces [--assets assets] [--out DIR]
       openprocyon-import --bake-surfaces [the same options]   (the release download)
"""

import argparse
import io
import json
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

from tools.bake_sky import default_assets, progress, replacing, srgb_to_linear
from tools.import_assets import user_dir

MOD = {
    "id": "baked-surfaces",
    "name": "Baked surfaces",
    "version": "1.0",
    "category": "Visuals",
    "description": "Relief maps for the Remaster look's surfaces, baked locally by tools/bake_surfaces.py (not retail).",
}

# Slopes are small (a few hundredths of luma per texel); stored times this so 8 bits keep their gradations. The shader
# divides it back out (`surface.wesl`).
SLOPE_GAIN = 3.0
# Relief scales in texels (Gaussian sigma) and their weights: the finest is about what the shader derives itself, the
# coarser ones carry the shapes it cannot see. PROVISIONAL: tuned by eye.
SCALES = ((0.6, 1.0), (2.0, 1.5), (5.0, 2.0))


def material_images(glb):
    """{material name: RGB uint8 array} of the base colour textures in one GLB."""
    raw = Path(glb).read_bytes()
    n = struct.unpack_from("<I", raw, 12)[0]
    j = json.loads(raw[20 : 20 + n])
    binary = raw[20 + n + 8 :]
    out = {}
    for m in j.get("materials", []):
        t = m.get("pbrMetallicRoughness", {}).get("baseColorTexture")
        if t is None or "name" not in m:
            continue
        bv = j["bufferViews"][
            j["images"][j["textures"][t["index"]]["source"]]["bufferView"]
        ]
        off = bv.get("byteOffset", 0)
        image = Image.open(io.BytesIO(binary[off : off + bv["byteLength"]]))
        out[m["name"]] = np.asarray(image.convert("RGB"))
    return out


def map_name(material):
    """The map's file stem for a material name (`surfaces.rs` `map_file` does the same)."""
    return material.lower().removesuffix(".tga")


def blur(h, sigma):
    """Gaussian blur that wraps at the edges (retail textures tile)."""
    r = max(1, int(np.ceil(sigma * 3)))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    for axis in (0, 1):
        h = sum(
            w * np.roll(h, i, axis) for i, w in zip(range(-r, r + 1), k, strict=True)
        )
    return h


def bake(rgb):
    """(H, W, 4) uint8 surface map of one colour texture."""
    lin = srgb_to_linear(rgb.astype(np.float32) / 255)
    h = lin @ np.array([0.299, 0.587, 0.114], np.float32)
    du = np.zeros_like(h)
    dv = np.zeros_like(h)
    for sigma, w in SCALES:
        b = blur(h, sigma)
        # central differences: + = brighter (higher) toward +u / +v (image columns / rows)
        du += w * (np.roll(b, -1, 1) - np.roll(b, 1, 1)) * 0.5
        dv += w * (np.roll(b, -1, 0) - np.roll(b, 1, 0)) * 0.5
    # crevices: lower than their surroundings, near and a little wider
    fine = blur(h, 0.6)
    cavity = np.clip((blur(h, 1.5) - fine) * 6 + (blur(h, 4.0) - fine) * 3, 0, 1)
    out = np.stack(
        [du * SLOPE_GAIN * 0.5 + 0.5, dv * SLOPE_GAIN * 0.5 + 0.5, cavity, h], -1
    )
    return np.clip(out * 255 + 0.5, 0, 255).astype(np.uint8)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--assets", type=Path, default=None)
    ap.add_argument("--out", type=Path, default=None, help="mod folder to write")
    args = ap.parse_args(argv)
    assets = args.assets or default_assets()
    out = args.out
    if out is None:
        d = user_dir()
        if not d:
            sys.exit("no XDG_DATA_HOME, HOME or APPDATA for the mod folder: pass --out")
        out = d / "mods" / MOD["id"]
    glbs = sorted((assets / "models").rglob("*.glb")) + sorted(
        (assets / "ships").rglob("*.glb")
    )
    if not glbs:
        sys.exit(f"no models in {assets}: import the game first")
    textures = {}
    for glb in glbs:
        for name, rgb in material_images(glb).items():
            textures.setdefault(map_name(name), rgb)
    dst_dir = out / "textures" / "surface"
    dst_dir.mkdir(parents=True, exist_ok=True)
    step = progress(len(textures))
    for name, rgb in sorted(textures.items()):
        with replacing(dst_dir / f"{name}.png") as part:
            Image.fromarray(bake(rgb), "RGBA").save(part, "PNG")
        step(name)
    # Written last, so a bake that failed or was stopped does not show in the Mods screen as a finished mod.
    (out / "mod.json").write_text(json.dumps(MOD, indent=2) + "\n")
    print(f"done: switch on '{MOD['name']}' in the Mods screen (or --mod {out})")


if __name__ == "__main__":
    main()
