"""Bake high-resolution cubemap skies from the player's imported battle skies (not retail: an optional visual upgrade).

Each retail sky is one 256x256 texture on a low-poly sphere that tiles it twice around the horizon. The baker keeps that
texture as the colour and density guide, sampled with the same direction -> (u, v) mapping as the sphere, and adds what
it lacks at modern resolutions: domain-warped filaments, dust lanes, fine grain and point stars. It writes a cubemap
(six square faces stacked vertically in +X -X +Y -Y +Z -Z order) as `models/sky/<sky>_cube.png`; the game draws it as a
skybox instead of the sphere when the file exists (`scenario.rs`, `view::hd_sky`).
The result is derived from the player's own copy, so it stays local: by default a mod folder `<user dir>/mods/hd-skies`,
switched on in the Mods screen. Needs only NumPy and Pillow.
usage: python3 -m tools.bake_sky [--assets assets] [--out DIR] [--size 2048] [--jobs N] [--split-stars] [skybox_07 ...]
"""

import argparse
import io
import json
import os
import struct
import sys
import tempfile
import zlib
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

from tools.import_assets import user_dir

MOD = {
    "id": "hd-skies",
    "name": "HD skies",
    "version": "1.0",
    "category": "Visuals",
    "description": "Battle skies rebaked locally at high resolution by tools/bake_sky.py (not retail).",
}

GRADS = np.array(
    [
        *[(1, 1, 0), (-1, 1, 0), (1, -1, 0), (-1, -1, 0), (1, 0, 1), (-1, 0, 1)],
        *[(1, 0, -1), (-1, 0, -1), (0, 1, 1), (0, -1, 1), (0, 1, -1), (0, -1, -1)],
    ],
    np.float32,
)


def read_glb(path):
    """(texture as RGB uint8 array, vertex positions, vertex UVs) of a one-mesh sky GLB."""
    raw = Path(path).read_bytes()
    n = struct.unpack_from("<I", raw, 12)[0]
    j = json.loads(raw[20 : 20 + n])
    binary = raw[20 + n + 8 :]

    def view(i):
        bv = j["bufferViews"][i]
        off = bv.get("byteOffset", 0)
        return binary[off : off + bv["byteLength"]]

    def accessor(i, k):
        a = j["accessors"][i]
        data = view(a["bufferView"])[a.get("byteOffset", 0) :]
        return np.frombuffer(data, np.float32, a["count"] * k).reshape(-1, k)

    prim = j["meshes"][0]["primitives"][0]["attributes"]
    image = Image.open(io.BytesIO(view(j["images"][0]["bufferView"])))
    return (
        np.array(image.convert("RGB")),
        accessor(prim["POSITION"], 3),
        accessor(prim["TEXCOORD_0"], 2),
    )


def azimuth_elevation(d):
    """Degrees, in the sky GLB's frame (x starboard, y up, -z forward): azimuth 0 ahead, 90 to starboard."""
    az = np.degrees(np.arctan2(d[..., 0], -d[..., 2]))
    el = np.degrees(np.arcsin(np.clip(d[..., 1], -1, 1)))
    return az, el


def fit_mapping(pos, uv):
    """How the sphere lays the texture on directions: u = offset - repeats * azimuth / 360 (retail spheres repeat the
    art 1, 2 or 3 times around the horizon) and v = slope * height + centre (height is the direction's up component).
    Fitted over the vertices away from the poles, whose UVs are irregular on some spheres. Returns
    (repeats, offset, slope, centre, worst residual) so a differently built sphere is noticed."""
    d = pos / np.linalg.norm(pos, axis=1, keepdims=True)
    az, _ = azimuth_elevation(d)
    side = np.abs(d[:, 1]) < 0.9
    slope, centre = np.polyfit(d[side, 1], uv[side, 1], 1)
    v_err = np.abs(slope * d[side, 1] + centre - uv[side, 1]).max()
    best = None
    for repeats in (1, 2, 3, 4):
        x = uv[side, 0] + repeats * az[side] / 360
        offset = np.angle(np.exp(2j * np.pi * x).mean()) / (2 * np.pi) % 1
        err = np.abs((x - offset + 0.5) % 1 - 0.5).max()
        if best is None or err < best[2]:
            best = (repeats, offset, err)
    repeats, offset, u_err = best
    return repeats, float(offset), float(slope), float(centre), float(max(v_err, u_err))


def face_dirs(face, n):
    """World directions (Bevy frame) of one cube face's texel centres, rows top to bottom. Bevy's skybox samples the
    cube with the world ray times (1, 1, -1), so the face layout is the usual cube-map one in that mirrored frame."""
    t = (np.arange(n, dtype=np.float32) + 0.5) / n * 2 - 1
    u, v = np.meshgrid(t, t)
    one = np.ones_like(u)
    s = [
        (one, -v, -u),
        (-one, -v, u),
        (u, one, v),
        (u, -one, -v),
        (u, -v, one),
        (-u, -v, -one),
    ][face]
    d = np.stack([s[0], s[1], -s[2]], -1)
    return d / np.linalg.norm(d, axis=-1, keepdims=True)


class Noise:
    """3D gradient noise on the unit sphere's directions: seamless across cube faces, deterministic per seed."""

    def __init__(self, seed):
        perm = np.random.default_rng(seed).permutation(256).astype(np.int32)
        self.perm = np.concatenate([perm, perm])

    def __call__(self, p):
        p = p.astype(np.float32)
        i = np.floor(p).astype(np.int32)
        f = p - i
        i &= 255
        w = f * f * f * (f * (f * 6 - 15) + 10)
        out = np.zeros(p.shape[:-1], np.float32)
        for c in range(8):
            o = np.array([(c >> 2) & 1, (c >> 1) & 1, c & 1], np.int32)
            h = self.perm[
                self.perm[self.perm[i[..., 0] + o[0]] + i[..., 1] + o[1]]
                + i[..., 2]
                + o[2]
            ]
            g = GRADS[h % 12]
            dot = np.sum(g * (f - o), axis=-1)
            wt = (
                (w[..., 0] if o[0] else 1 - w[..., 0])
                * (w[..., 1] if o[1] else 1 - w[..., 1])
                * (w[..., 2] if o[2] else 1 - w[..., 2])
            )
            out += wt * dot
        return out

    def fbm(self, d, freq, octaves, gain=0.5, ridged=False):
        total = np.zeros(d.shape[:-1], np.float32)
        amp, norm = 1.0, 0.0
        for k in range(octaves):
            # each octave shifted so the lattice never lines up between octaves
            n = self(d * freq * 2**k + np.float32(17.3 * k))
            total += amp * ((1 - np.abs(n) * 1.6) ** 2 if ridged else n)
            norm += amp
            amp *= gain
        return total / norm


def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def prepare_guide(tex):
    """The retail texture in linear light at 4x, smoothed: its 256-texel blocks and compression noise go, colours and
    shapes stay."""
    h, w = tex.shape[:2]
    big = Image.fromarray(tex).resize((w * 4, h * 4), Image.Resampling.LANCZOS)
    big = big.filter(ImageFilter.GaussianBlur(3))
    return srgb_to_linear(np.asarray(big, np.float32) / 255)


def sample(img, u, v):
    """Bilinear sample, u wrapping (the sphere repeats the texture), v clamped."""
    h, w = img.shape[:2]
    x = (u % 1) * w - 0.5
    y = np.clip(v * h - 0.5, 0, h - 1)
    x0 = np.floor(x).astype(np.int32)
    y0 = np.floor(y).astype(np.int32)
    fx = (x - x0)[..., None]
    fy = (y - y0)[..., None]
    x0 %= w
    x1 = (x0 + 1) % w
    y1 = np.minimum(y0 + 1, h - 1)
    top = img[y0, x0] * (1 - fx) + img[y0, x1] * fx
    bot = img[y1, x0] * (1 - fx) + img[y1, x1] * fx
    return top * (1 - fy) + bot * fy


def luminance(c):
    return c[..., 0] * 0.2126 + c[..., 1] * 0.7152 + c[..., 2] * 0.0722


def smooth_fields(d, noise):
    """The slowly varying noise fields (domain warp, dust, haze) for directions d; cheap enough to bake at a quarter
    of the face resolution and scale up."""
    return np.stack(
        [
            noise.fbm(d, 2.5, 4) * 0.035 + noise.fbm(d + 5.1, 14, 3) * 0.008,
            noise.fbm(d + 9.7, 2.5, 4) * 0.02 + noise.fbm(d + 3.3, 14, 3) * 0.005,
            noise.fbm(d + 4.4, 4, 5, gain=0.55),
            noise.fbm(d + 2.2, 1.6, 4),
        ],
        -1,
    )


# Each cube face is baked in this many horizontal bands (the unit of parallel work, so 6 faces keep every core busy; the
# count is fixed, not the job count, so the pixels never depend on --jobs).
BANDS = 4
CHUNK = 1 << 13  # texels per piece of noise work


def band_rows(size, band):
    return size * band // BANDS, size * (band + 1) // BANDS


def direction_fields(face, size, noise, r0=0, r1=None):
    """Everything the nebula needs that depends only on the look direction and the seed (so is the same for every sky):
    (size, size, 6) float32 with the warp x/y, dust, haze (already shaped), ridged filaments and grain."""
    r1 = size if r1 is None else r1
    d = face_dirs(face, size)[r0:r1]
    smooth = upscale(smooth_fields(face_dirs(face, max(size // 4, 2)), noise), size)[
        r0:r1
    ]
    ridge = np.empty(d.shape[:2], np.float32)
    grain = np.empty(d.shape[:2], np.float32)
    # in pieces small enough for the cache: the noise makes many temporaries, and on big arrays the bake is limited by
    # memory bandwidth, which the parallel processes share
    step = max(1, CHUNK // size)
    for r in range(0, len(d), step):
        c = d[r : r + step]
        ridge[r : r + step] = noise.fbm(c + 1.7, 7, 6, gain=0.55, ridged=True)
        grain[r : r + step] = 1 + 0.06 * noise.fbm(c + 7.9, 160, 2)
    haze = np.clip(smooth[..., 3] * 0.5 + 0.5, 0, 1) ** 3
    return np.concatenate(
        [smooth[..., :3], haze[..., None], ridge[..., None], grain[..., None]], -1
    ).astype(np.float32)


def nebula(d, fields, guide, rows, mapping, mean):
    """Linear RGB and density of the nebula for directions d (N, 3); fields holds `direction_fields` for the same
    directions (warp x/y, dust, haze, ridged filaments, grain: all independent of the sky), rows the guide's mean
    colour per row."""
    repeats, offset, slope, centre = mapping
    az, _ = azimuth_elevation(d)
    u = offset - repeats * az / 360
    # The sphere's own vertical layout: it squeezes the art's whole height into its bright band (mdb_to_glb.py), and
    # the battle camera mostly looks down, so this keeps the nebula where the player sees it in classic.
    v = centre + slope * d[..., 1]
    # Domain warp: the guide is sampled a little off its true place, by large and small swirls, which turns its
    # soft blobs into folded, streaky gas without moving any feature far from where the retail sky has it.
    wx, wy, dust, haze, ridge, grain = (fields[..., k] for k in range(6))
    base = sample(guide, u + wx, v + wy)
    # Toward the poles u converges to a point: fade to the row's average colour so nothing pinches there.
    flat = np.clip((np.sqrt(np.maximum(1 - d[..., 1] ** 2, 0)) - 0.08) / 0.4, 0, 1)
    row = rows[np.clip(((v + wy) * len(rows)).astype(np.int32), 0, len(rows) - 1)]
    base = row + (base - row) * (flat * flat * (3 - 2 * flat))[..., None]
    dens = np.clip(luminance(base) / max(luminance(mean) * 2.5, 1e-4), 0, 1)
    # Filaments: ridged noise, strongest where the gas is of middling density (cores stay bright, voids stay dark).
    # Being 3D noise on the direction it is the same size every way, which hides the art's vertical stretch.
    strength = 0.95 * np.sqrt(dens) * (1 - 0.5 * dens)
    detail = 1 + strength * (ridge * 2.2 - 1.0)
    # Dust lanes: dark absorbing bands across the gas.
    lanes = np.clip((dust - 0.08) / 0.25, 0, 1) ** 2 * np.clip(dens * 3, 0, 1)
    # Fine grain at about texel scale, so the gas does not look smooth at 1:1.
    col = base * (np.clip(detail, 0.15, 3) * (1 - 0.65 * lanes) * grain)[..., None]
    # Bright cores glow a little hotter (whiter) than the guide, for depth.
    core = np.clip(dens - 0.6, 0, 1)[..., None] ** 2
    col = col + core * luminance(col)[..., None] * 0.6
    # Where the retail art is black, a very faint haze in the sky's own mean colour keeps space from looking empty.
    col = col + mean * (0.08 * haze)[..., None]
    return col, dens


def upscale(field, n):
    """Bilinear resize of a (m, m, k) float field to (n, n, k)."""
    return np.stack(
        [
            np.asarray(
                Image.fromarray(field[..., k]).resize((n, n), Image.Resampling.BILINEAR)
            )
            for k in range(field.shape[-1])
        ],
        -1,
    )


def star_field(seed, count):
    """Unit directions, brightness (linear, before nebula dimming) and colour of the stars."""
    rng = np.random.default_rng(seed + 1000)
    d = rng.normal(size=(count, 3)).astype(np.float32)
    d /= np.linalg.norm(d, axis=1, keepdims=True)
    # Steep power law: very many faint stars, a handful of bright ones.
    lum = 0.04 * rng.pareto(2.1, count).astype(np.float32) + 0.012
    lum = np.minimum(lum, 4)
    # Blackbody-ish palette from red-orange through white to blue.
    t = rng.beta(2.2, 2.0, count).astype(np.float32)
    warm = np.array([1.0, 0.72, 0.48], np.float32)
    white = np.array([1.0, 0.97, 0.94], np.float32)
    blue = np.array([0.68, 0.80, 1.0], np.float32)
    col = np.where(
        (t < 0.5)[:, None],
        warm + (white - warm) * (t[:, None] * 2),
        white + (blue - white) * ((t[:, None] - 0.5) * 2),
    )
    return d, lum, col


def draw_stars(img, face, n, stars):
    """Adds the stars to one face (n x n x 3, linear) as small Gaussians; ones near an edge are drawn on every face
    they touch, so none is cut at a seam."""
    d, lum, col = stars
    s = d * np.array([1, 1, -1], np.float32)  # into the cube's sampling frame
    axis, sign = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)][face]
    ma = s[:, axis] * sign
    front = ma > 0.3
    s, lum, col, ma = s[front], lum[front], col[front], ma[front]
    # inverse of face_dirs: the face's (u, v) in -1..1
    x, y, z = s[:, 0] / ma, s[:, 1] / ma, s[:, 2] / ma
    u, v = [(-z, -y), (z, -y), (x, z), (x, -z), (x, -y), (-x, -y)][face]
    px = (u + 1) / 2 * n - 0.5
    py = (v + 1) / 2 * n - 0.5
    scale = n / 2048
    for i in np.argsort(lum):
        # faint stars about a texel across; bright ones get a soft halo, the brightest small cross spikes
        sigma = (0.55 + 0.35 * min(lum[i], 1)) * max(scale, 0.5)
        halo_s = sigma * 4 * min(lum[i], 3) if lum[i] > 0.6 else 0
        ls = 3.5 * scale * lum[i] if lum[i] > 1.5 else 0
        # the patch must hold the whole star (core, halo, spikes) or its edge shows as a square
        r = int(np.ceil(max(sigma * 4, halo_s * 3.5, ls * 4)))
        cx, cy = px[i], py[i]
        x0, x1 = int(np.floor(cx)) - r, int(np.floor(cx)) + r + 2
        y0, y1 = int(np.floor(cy)) - r, int(np.floor(cy)) + r + 2
        if x1 <= 0 or y1 <= 0 or x0 >= n or y0 >= n:
            continue
        x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, n), min(y1, n)
        gx = np.arange(x0, x1, dtype=np.float32) - cx
        gy = np.arange(y0, y1, dtype=np.float32) - cy
        dx, dy = np.meshgrid(gx, gy)
        r2 = dx * dx + dy * dy
        k = np.exp(-r2 / (2 * sigma * sigma))
        if lum[i] > 0.6:
            k = k + 0.07 * np.exp(-r2 / (2 * halo_s * halo_s))
        if lum[i] > 1.5:
            spikes = np.exp(-(dx * dx) / 0.5) * np.exp(-np.abs(dy) / ls)
            spikes += np.exp(-(dy * dy) / 0.5) * np.exp(-np.abs(dx) / ls)
            # faint and short: a hint of a diffraction cross, not a plus sign
            k = k + 0.035 * spikes
        # fade to exactly zero at the patch edge so no cut-off can show
        k = k * np.clip(1 - np.sqrt(r2) / (r + 1), 0, 1) ** 0.5
        img[y0:y1, x0:x1] += k[..., None] * (lum[i] * col[i])


# The stars layer is stored as 16-bit linear light divided by this, so the brightest stars exceed 1 (and bloom) when the
# game multiplies it back (`stars.gain` of the sky block, same default).
STAR_GAIN = 16.0


def write_png16(path, img):
    """Writes (H, W, 3) uint16 as a 16-bit RGB PNG (Pillow cannot write 16-bit colour)."""
    h, w, _ = img.shape
    raw = b"".join(b"\0" + row.astype(">u2").tobytes() for row in img)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 16, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


def read_png16(path):
    """Reads back what write_png16 wrote: (H, W, 3) uint16."""
    data = Path(path).read_bytes()[8:]
    idat, pos = b"", 0
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind, body = data[pos + 4 : pos + 8], data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            w, h = struct.unpack(">II", body[:8])
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    rows = np.frombuffer(zlib.decompress(idat), np.uint8).reshape(h, 1 + w * 6)
    return rows[:, 1:].copy().view(">u2").reshape(h, w, 3).astype(np.uint16)


class FaceBase:
    """What one band of a cube face needs that is the same for every sky: directions, noise fields and the undimmed
    stars."""

    def __init__(self, face, size, seed, stars=60000, band=0):
        noise = Noise(seed)
        self.face, self.size, self.seed, self.band = face, size, seed, band
        r0, r1 = band_rows(size, band)
        self.d = face_dirs(face, size)[r0:r1]
        self.fields = direction_fields(face, size, noise, r0, r1)
        full = np.zeros((size, size, 3), np.float32)
        draw_stars(
            full, face, size, star_field(seed, int(stars * (size / 2048) ** 0.5))
        )
        self.stars = full[r0:r1].copy()


class SkyGuide:
    """The per-sky inputs: the smoothed retail art and how the sphere maps it."""

    def __init__(self, tex, pos, uv):
        *self.mapping, _ = fit_mapping(pos, uv)
        self.guide = prepare_guide(tex)
        self.rows = self.guide.mean(1)
        self.mean = srgb_to_linear(tex.reshape(-1, 3).astype(np.float32) / 255).mean(0)


def shade_face(base, sky, split_stars=False):
    """(nebula band uint8, stars band uint16 or None) of one sky on the face band `base` was prepared for."""
    size, rows = base.size, len(base.d)
    col = np.empty((rows, size, 3), np.float32)
    dens = np.empty((rows, size), np.float32)
    step = max(1, CHUNK // size)
    for r in range(0, rows, step):
        col[r : r + step], dens[r : r + step] = nebula(
            base.d[r : r + step],
            base.fields[r : r + step],
            sky.guide,
            sky.rows,
            sky.mapping,
            sky.mean,
        )
    # thick gas hides the stars behind it
    star = base.stars * (1 - 0.6 * np.clip(dens, 0, 1) ** 0.7)[..., None]
    star_face = None
    if split_stars:
        star_face = np.clip(star / STAR_GAIN * 65535 + 0.5, 0, 65535).astype(np.uint16)
    else:
        col += star
    out = linear_to_srgb(col) * 255
    # triangular dither: no banding in the dark gradients (seeded per face so faces can be baked apart)
    rng = np.random.default_rng([base.seed + 2000, base.face, base.band])
    out += rng.random(out.shape, np.float32) - rng.random(out.shape, np.float32)
    return np.clip(out + 0.5, 0, 255).astype(np.uint8), star_face


def bake(tex, pos, uv, size, seed=1, stars=60000):
    """The six faces (6 * size x size x 3 uint8, stacked +X -X +Y -Y +Z -Z) for one retail sky."""
    return bake_layers(tex, pos, uv, size, seed, stars)[0]


def bake_layers(tex, pos, uv, size, seed=1, stars=60000, split_stars=False):
    """(nebula, stars): the nebula faces as `bake` returns them and, with split_stars, the stars alone (6 * size x size x 3
    uint16, linear light over STAR_GAIN, already dimmed by the gas in front of them) left out of the nebula; else None."""
    sky = SkyGuide(tex, pos, uv)
    faces, star_faces = [], []
    for face in range(6):
        for band in range(BANDS):
            nebula_face, star_face = shade_face(
                FaceBase(face, size, seed, stars, band), sky, split_stars
            )
            faces.append(nebula_face)
            star_faces.append(star_face)
    return np.concatenate(faces, 0), (
        np.concatenate(star_faces, 0) if split_stars else None
    )


def load_sky(glb):
    """(SkyGuide, None) of a sky GLB, or (None, reason) when its sphere does not follow the retail mapping."""
    tex, pos, uv = read_glb(glb)
    *_, err = fit_mapping(pos, uv)
    if err > 0.2:
        return (
            None,
            f"{glb.stem}: sphere UVs do not follow the retail mapping (error {err:.3f}), skipped",
        )
    return SkyGuide(tex, pos, uv), None


def bake_face_job(job, glbs, tmp, size, seed, split_stars):
    """Stage 1, one process per cube face: the direction-only fields once, then every sky through them. Faces are
    left in `tmp` as .npy files for `assemble_job`. `job` is (face, band)."""
    face, band = job
    base = FaceBase(face, size, seed, band=band)
    for glb in glbs:
        sky, _ = load_sky(glb)
        if sky is None:
            continue
        nebula_face, star_face = shade_face(base, sky, split_stars)
        np.save(tmp / f"{glb.stem}.{face}.{band}.npy", nebula_face)
        if star_face is not None:
            np.save(tmp / f"{glb.stem}.{face}.{band}.stars.npy", star_face)
    return job


def assemble_job(glb, out, tmp, split_stars):
    """Stage 2, one process per sky: stacks the six faces and writes the PNGs."""
    sky, why = load_sky(glb)
    if sky is None:
        return why
    img = np.concatenate(
        [
            np.load(tmp / f"{glb.stem}.{f}.{k}.npy")
            for f in range(6)
            for k in range(BANDS)
        ],
        0,
    )
    dst = out / "models" / "sky" / f"{glb.stem}_cube.png"
    dst.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(img, "RGB").save(dst, compress_level=6)
    note = ""
    if split_stars:
        star_img = np.concatenate(
            [
                np.load(tmp / f"{glb.stem}.{f}.{k}.stars.npy")
                for f in range(6)
                for k in range(BANDS)
            ],
            0,
        )
        write_png16(dst.with_name(f"{glb.stem}_stars.png"), star_img)
        note = " + stars"
    return f"{dst} ({dst.stat().st_size >> 20} MB{note})"


def default_assets():
    """PROCYON_ASSETS, else this checkout's assets/ (the baker lives in tools/), else the per-user folder."""
    if os.environ.get("PROCYON_ASSETS"):
        return Path(os.environ["PROCYON_ASSETS"])
    here = Path(__file__).resolve().parent.parent / "assets"
    if here.is_dir():
        return here
    d = user_dir()
    return d / "assets" if d else here


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "skies", nargs="*", help="sky names (default: every models/sky/*.glb)"
    )
    ap.add_argument("--assets", type=Path, default=None)
    ap.add_argument("--out", type=Path, default=None, help="mod folder to write")
    ap.add_argument("--size", type=int, default=2048, help="cube face size in texels")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument(
        "--split-stars",
        action="store_true",
        help="bake the stars into their own 16-bit <sky>_stars.png (for the `sky` block's twinkling stars) and leave them out of <sky>_cube.png",
    )
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    args = ap.parse_args(argv)
    assets = args.assets or default_assets()
    out = args.out
    if out is None:
        d = user_dir()
        if not d:
            sys.exit("no XDG_DATA_HOME, HOME or APPDATA for the mod folder: pass --out")
        out = d / "mods" / MOD["id"]
    glbs = sorted((assets / "models" / "sky").glob("*.glb"))
    if args.skies:
        glbs = [g for g in glbs if g.stem in args.skies]
    if not glbs:
        sys.exit(f"no sky models in {assets / 'models' / 'sky'}: import the game first")
    out.mkdir(parents=True, exist_ok=True)
    (out / "mod.json").write_text(json.dumps(MOD, indent=2) + "\n")
    with (
        tempfile.TemporaryDirectory(dir=out) as t,
        ProcessPoolExecutor(args.jobs) as pool,
    ):
        tmp = Path(t)
        n = len(glbs)
        jobs = [(f, k) for k in range(BANDS) for f in range(6)]
        for face, band in pool.map(
            bake_face_job,
            jobs,
            [glbs] * len(jobs),
            [tmp] * len(jobs),
            [args.size] * len(jobs),
            [args.seed] * len(jobs),
            [args.split_stars] * len(jobs),
        ):
            print(f"face {face} band {band}: {n} skies shaded", flush=True)
        for line in pool.map(
            assemble_job, glbs, [out] * n, [tmp] * n, [args.split_stars] * n
        ):
            print(line, flush=True)
    print(f"done: switch on '{MOD['name']}' in the Mods screen (or --mod {out})")


if __name__ == "__main__":
    main()
