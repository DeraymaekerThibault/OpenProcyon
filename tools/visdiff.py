"""Screenshots of the merge base against this checkout (#535): one line per scene, diff pictures in local/visdiff/.
usage: python3 -m tools.visdiff [--base REF] [--scene NAME ...] [--list]
The base is built (`fast` profile) in a warm worktree ../openprocyon-visdiff-base (made once with tools/worktree.sh, then moved
to REF detached); this checkout is built in place. Every scene is a fixed-step script (--fixed-step: same seed, one tick per
frame), rendered offscreen, so an unchanged scene gives identical pixels. Nothing is committed or uploaded (hard rule 1).
Prints `same <scene>` or `CHANGED <scene> 3.1% px, bbox x0,y0-x1,y1, diff local/visdiff/<scene>_diff.png`; exit 1 when any changed."""

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image

from tools.paths import REPO

OUT = REPO / "local/visdiff"
BASE_WT = REPO.parent / "openprocyon-visdiff-base"
# A channel moving by this much or less is not a change (none is expected in a fixed-step run; this only absorbs dithering).
TOLERANCE = 0
SCENES = {
    "battle": ["--map", "red_rover", "--shot-frame", "300"],
    "fx": ["--map", "red_rover", "--script", "fxtest", "--shot-frame", "150"],
    "damage": ["--map", "red_rover", "--script", "damage", "--shot-frame", "120"],
    "skirmish-menu": ["--menu", "skirmish", "--shot-frame", "60"],
}


def git(*args, cwd=REPO):
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def base_tree(ref):
    """The base worktree, detached at `ref`."""
    if not BASE_WT.exists():
        subprocess.run(
            ["tools/worktree.sh", "new", "visdiff-base", str(BASE_WT)],
            cwd=REPO,
            check=True,
        )
    git("checkout", "-q", "--detach", ref, cwd=BASE_WT)
    return BASE_WT


def build(tree):
    """The `fast` binary of `tree` (outside the run's time cap: a first build in a fresh tree takes minutes)."""
    env = {k: v for k, v in os.environ.items() if k != "CARGO_TARGET_DIR"}
    subprocess.run(
        ["cargo", "build", "--profile", "fast", "-q"],
        cwd=Path(tree, "game"),
        env=env,
        check=True,
    )


def render(tree, scene, png):
    """One fixed-step picture from `tree`'s build, offscreen, with default settings; the run is capped and killed with its game."""
    with tempfile.TemporaryDirectory() as xdg:
        env = {
            k: v
            for k, v in os.environ.items()
            if k not in ("DISPLAY", "WAYLAND_DISPLAY", "CARGO_TARGET_DIR")
        }
        env["XDG_DATA_HOME"] = xdg
        cap = [
            "systemd-run",
            "--user",
            "--scope",
            "-q",
            "-p",
            "RuntimeMaxSec=300",
            "-p",
            "MemoryMax=4G",
            "-p",
            "MemorySwapMax=0",
        ]
        cmd = [*cap, "tools/shot.sh", str(png), *SCENES[scene], "--fixed-step"]
        run = subprocess.run(cmd, cwd=tree, env=env, capture_output=True, text=True)
    if run.returncode or not png.exists():
        tail = (run.stdout + run.stderr).strip().splitlines()[-3:]
        raise RuntimeError(f"{scene}: no picture from {tree} ({'; '.join(tail)})")


def compare(a, b, diff):
    """(changed fraction, bbox or None); writes `diff` (the branch picture greyed, changed pixels red) when something changed."""
    pa = np.asarray(Image.open(a).convert("RGB"), dtype=np.int16)
    pb = np.asarray(Image.open(b).convert("RGB"), dtype=np.int16)
    if pa.shape != pb.shape:
        return 1.0, (0, 0, pb.shape[1] - 1, pb.shape[0] - 1)
    changed = (np.abs(pa - pb) > TOLERANCE).any(axis=2)
    if not changed.any():
        return 0.0, None
    ys, xs = np.nonzero(changed)
    grey = pb.mean(axis=2, keepdims=True).repeat(3, axis=2) * 0.4
    grey[changed] = (255, 0, 0)
    Image.fromarray(grey.astype(np.uint8)).save(diff)
    return changed.mean(), (xs.min(), ys.min(), xs.max(), ys.max())


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument(
        "--base", help="ref to compare against (default: merge base with origin/main)"
    )
    p.add_argument(
        "--scene",
        action="append",
        choices=sorted(SCENES),
        help="scene to render (repeatable; default: all)",
    )
    p.add_argument("--list", action="store_true", help="print the scenes and stop")
    args = p.parse_args(argv)
    if args.list:
        print("\n".join(f"{k}: {' '.join(v)}" for k, v in SCENES.items()))
        return 0
    ref = git("rev-parse", args.base or git("merge-base", "HEAD", "origin/main"))
    OUT.mkdir(parents=True, exist_ok=True)
    trees = {"base": base_tree(ref), "branch": REPO}
    print(
        f"visdiff: base {ref[:9]} vs {git('rev-parse', '--short', 'HEAD')} (+ uncommitted changes)",
        flush=True,
    )
    for tree in trees.values():
        build(tree)
    changed = 0
    for scene in args.scene or SCENES:
        pics = {side: OUT / f"{scene}_{side}.png" for side in trees}
        for side, tree in trees.items():
            pics[side].unlink(missing_ok=True)
            render(tree, scene, pics[side])
        diff = OUT / f"{scene}_diff.png"
        diff.unlink(missing_ok=True)
        frac, box = compare(pics["base"], pics["branch"], diff)
        if box is None:
            print(f"same     {scene}", flush=True)
            continue
        changed += 1
        print(
            f"CHANGED  {scene} {frac:.2%} px ({round(frac * 1280 * 720)} px), bbox {box[0]},{box[1]}-{box[2]},{box[3]}, diff {diff.relative_to(REPO)}",
            flush=True,
        )
    return 1 if changed else 0


if __name__ == "__main__":
    sys.exit(main())
