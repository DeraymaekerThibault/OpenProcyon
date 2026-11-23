"""Export a ship's physics constants from its WOT to JSON for the game.

usage: python -m tools.export_ship <game_root> Ship_Navy_Cutter <out.json>
Values are raw WOT literals (units not established); each carries its source line.
"""

from pathlib import Path

from tools import cli
from tools.jsonio import dump
from tools.wot import parse_lines, properties, read_wot

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
    lines = read_wot(root, name)
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
    root, name, out = cli.args(__doc__, 3)
    Path(out).write_text(dump(export(root, name)), encoding="utf-8")
    print("wrote", out)
