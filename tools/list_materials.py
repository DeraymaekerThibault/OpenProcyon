"""List the glTF material names of an import grouped by the remaster `surfaces` rule that wins, so a rule change is checked against
every real material (substring misfires, guns or scenery left at the default).
usage: python -m tools.list_materials [--assets DIR] [--look FILE]
Reads every .glb under <assets>/ships and <assets>/models; the default look is mods/remaster/data/render/look.json. Prints one block per
rule (in rule order), then `skipped` and `default`. Same matching as game/src/render/surfaces.rs: lower-case substring, skip first,
first rule wins. Local use only: the output names game materials, so never paste it into a commit, issue or CI artifact."""

import json
import struct
import sys
from pathlib import Path

from tools.import_assets import find_assets
from tools.paths import REPO

LOOK = REPO / "mods/remaster/data/render/look.json"


def glb_materials(path):
    """The material names in a .glb's JSON chunk."""
    data = Path(path).read_bytes()
    if data[:4] != b"glTF":
        return []
    length, kind = struct.unpack_from("<I4s", data, 12)
    if kind != b"JSON":
        return []
    doc = json.loads(data[20 : 20 + length])
    return [m["name"] for m in doc.get("materials", []) if m.get("name")]


def classify(surfaces, name):
    """`skipped`, the index of the winning rule, or `default`."""
    n = name.lower()
    if any(s in n for s in surfaces.get("skip", [])):
        return "skipped"
    for i, rule in enumerate(surfaces.get("rules", [])):
        if any(m in n for m in rule["match"]):
            return i
    return "default"


def group(surfaces, names):
    """{rule index | 'skipped' | 'default': sorted unique names}."""
    out = {}
    for name in set(names):
        out.setdefault(classify(surfaces, name), []).append(name)
    return {k: sorted(v, key=str.lower) for k, v in out.items()}


def main(argv):
    args = dict(zip(argv[::2], argv[1::2], strict=False))
    if set(args) - {"--assets", "--look"}:
        sys.exit(__doc__)
    assets = Path(args["--assets"]) if "--assets" in args else find_assets()
    if not assets or not (assets / "ships").is_dir():
        sys.exit("no imported assets: pass --assets DIR or import the game first")
    surfaces = json.loads(Path(args.get("--look", LOOK)).read_text(encoding="utf-8"))[
        "surfaces"
    ]
    names = [
        n
        for sub in ("ships", "models")
        for f in sorted((assets / sub).rglob("*.glb"))
        for n in glb_materials(f)
    ]
    groups = group(surfaces, names)
    rules = surfaces.get("rules", [])
    for key in [*range(len(rules)), "default", "skipped"]:
        found = groups.get(key, [])
        label = (
            f"rule {key} ({', '.join(rules[key]['match'][:4])}, ...)"
            if isinstance(key, int)
            else key
        )
        print(f"{label}: {len(found)}")
        for name in found:
            print(f"  {name}")


if __name__ == "__main__":
    main(sys.argv[1:])
