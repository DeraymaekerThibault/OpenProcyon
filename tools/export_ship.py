"""Export a ship's physics constants from its WOT to JSON for the game.

usage: python -m tools.export_ship <game_root> Ship_Navy_Cutter <out.json>
Values are raw WOT literals (units not established); each carries its source line.
"""

import json
import sys
from pathlib import Path

from tools.source_text import read_source_lines
from tools.wot import parse_lines, properties

KEYS = (
    "Mass",
    "MaxThrust",
    "MaxSpeed",
    "RotationalFriction",
    "MaxAngularAcceleration",
    "CenterOfMass",
    "MinExtents",
    "MaxExtents",
    "Core Damage Section Max HitPoints",
)
# Ship_Civilian_Lifeboat authors a bounding sphere instead of a box; the game falls back to the section bounds.
OPTIONAL = ("MinExtents", "MaxExtents")


def export(root, name):
    lines = read_source_lines(
        Path(root), Path("WorldObjects") / f"{name}.wot", encoding="utf-8"
    )
    nodes, problems = parse_lines(lines)
    if problems:
        raise ValueError(problems[:3])
    out = {}
    for p in properties(nodes):
        if p["name"] in KEYS:
            if p["name"] in out:
                raise ValueError(f"duplicate {p['name']}")
            out[p["name"]] = dict(value=p["value"], line=p["source"]["line"])
    missing = [k for k in KEYS if k not in out and k not in OPTIONAL]
    if missing:
        raise ValueError(f"missing {missing}")
    return dict(entity=name, source=f"WorldObjects/{name}.wot", properties=out)


if __name__ == "__main__":
    root, name, out = sys.argv[1:4]
    Path(out).write_text(json.dumps(export(root, name), indent=1))
    print("wrote", out)
