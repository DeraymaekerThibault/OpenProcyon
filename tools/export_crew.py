"""Crew roster: every Crew_*.wot -> assets/data/crew.json {id: {race, species, name_id, name, points, skills{active skill: ability}, portrait}}.
The game does not use crew yet; this is the data for fleet building and ship skills later.
usage: python3 -m tools.export_crew <game_root> <assets dir>"""

import json
import sys
from pathlib import Path

from tools.export_maps import strings
from tools.gamefs import ci_files
from tools.wot import parse_text, properties

SKILLS = (
    "Leadership",
    "Navigation",
    "Spotting",
    "Engineering",
    "Rigging",
    "Combat",
    "Gunnery",
)


def parse(text, names):
    nodes, _ = parse_text(text)
    f = {}
    for n in properties(nodes):
        f.setdefault(n["name"], n["value"])
    name_id = f.get("Crew Name String ID")
    return {
        "race": f.get("Crew Race"),
        "species": f.get("Species"),
        "name_id": name_id,
        "name": names.get(name_id, name_id),
        "points": f.get("Point Value", 0),
        "portrait": f.get("Talking Head Texture"),
        "skills": {
            s.lower(): f.get(f"{s} Skill Ability Value", 0)
            for s in SKILLS
            if f.get(f"{s} Skill Active", False)
        },
    }


def main(root, out):
    names = strings(root)
    crew = {
        f.stem: parse(f.read_text(encoding="latin1"), names)
        for f in ci_files(Path(root) / "WorldObjects", "Crew_*.wot")
    }
    (Path(out) / "data").mkdir(parents=True, exist_ok=True)
    (Path(out) / "data/crew.json").write_text(json.dumps(crew, indent=1))
    print(len(crew), "crew")


if __name__ == "__main__":
    main(*sys.argv[1:3])
