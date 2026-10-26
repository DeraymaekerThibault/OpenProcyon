"""Compare two asset trees by meaning, not bytes: JSON by parsed value (floats within a tolerance), images by pixels,
everything else byte for byte. Used to check that a fresh import reproduces the committed assets.
Usage: python3 -m tools.compare_assets <expected dir> <actual dir> [--detail N]
"""

import json
import math
import sys
from collections import Counter
from pathlib import Path

from PIL import Image

IMAGES = {".png", ".tga", ".dds", ".jpg"}


def files(root):
    return {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}


def json_diff(a, b, path="", tol=1e-4):
    """First differing JSON path, or None."""
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(a.keys() | b.keys()):
            if k not in a or k not in b:
                return f"{path}/{k} only in {'actual' if k in b else 'expected'}"
            d = json_diff(a[k], b[k], f"{path}/{k}", tol)
            if d:
                return d
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return f"{path} length {len(a)} != {len(b)}"
        for i, (x, y) in enumerate(zip(a, b, strict=True)):
            d = json_diff(x, y, f"{path}[{i}]", tol)
            if d:
                return d
        return None
    if (
        isinstance(a, (int, float))
        and isinstance(b, (int, float))
        and not isinstance(a, bool)
        and not isinstance(b, bool)
    ):
        return (
            None
            if math.isclose(a, b, rel_tol=tol, abs_tol=tol)
            else f"{path} {a} != {b}"
        )
    return None if a == b else f"{path} {str(a)[:60]!r} != {str(b)[:60]!r}"


def differs(a, b):
    """Why two files differ in meaning, or None when they match."""
    ext = a.suffix.lower()
    if ext == ".json":
        try:
            return json_diff(json.loads(a.read_text()), json.loads(b.read_text()))
        except ValueError as e:
            return f"unreadable JSON: {e}"
    if ext in IMAGES:
        x, y = Image.open(a).convert("RGBA"), Image.open(b).convert("RGBA")
        if x.size != y.size:
            return f"size {x.size} != {y.size}"
        return None if x.tobytes() == y.tobytes() else "pixels differ"
    return None if a.read_bytes() == b.read_bytes() else "bytes differ"


def compare(expected, actual):
    """{"only_expected": [...], "only_actual": [...], "differ": {rel: reason}}"""
    e, a = files(expected), files(actual)
    differ = {}
    for rel in sorted(e & a):
        why = differs(expected / rel, actual / rel)
        if why:
            differ[rel] = why
    return {
        "only_expected": sorted(e - a),
        "only_actual": sorted(a - e),
        "differ": differ,
    }


def folder(path):
    return "/".join(path.split("/")[:-1][:2]) or "."


def main(expected, actual, detail=5):
    r = compare(Path(expected), Path(actual))
    for key, label in (
        ("only_expected", f"only in {expected}"),
        ("only_actual", f"only in {actual}"),
        ("differ", "differ"),
    ):
        items = list(r[key])
        print(f"== {label}: {len(items)}")
        for g, n in Counter(map(folder, items)).most_common():
            examples = [p for p in items if folder(p) == g][:detail]
            print(f"  {n:5} {g}")
            for p in examples:
                print(
                    f"          {p}"
                    + (f": {r['differ'][p]}" if key == "differ" else "")
                )
    return 1 if any(r.values()) else 0


if __name__ == "__main__":
    args = sys.argv[1:]
    n = 5
    if "--detail" in args:
        i = args.index("--detail")
        n = int(args[i + 1])
        del args[i : i + 2]
    sys.exit(main(*args, detail=n))
