"""Crew roster: every Crew_*.wot -> assets/data/crew.json {id: {race, species, name_id, name, points, skills{active skill: ability}, portrait}}.
The game does not use crew yet; this is the data for fleet building and ship skills later.
usage: python3 -m tools.export_crew <game_root> <assets dir>"""

import json
import re
import sys
from pathlib import Path

from tools.export_maps import strings

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
    def g(pat):
        return (re.search(pat, text) or [None, None])[1]

    name_id = g(r"Crew Name String ID String '([^']*)'")
    return {
        "race": g(r"Crew Race String '([^']*)'"),
        "species": g(r"Species String '([^']*)'"),
        "name_id": name_id,
        "name": names.get(name_id, name_id),
        "points": int(g(r"Point Value Int (-?\d+)") or 0),
        "portrait": g(r"Talking Head Texture String '([^']*)'"),
        "skills": {
            s.lower(): int(g(rf"{s} Skill Ability Value Int (-?\d+)") or 0)
            for s in SKILLS
            if g(rf"{s} Skill Active Bool (\w+)") == "True"
        },
    }


def main(root, out):
    names = strings(root)
    crew = {
        f.stem: parse(f.read_text(encoding="latin1"), names)
        for f in sorted((Path(root) / "WorldObjects").glob("Crew_*.wot"))
    }
    (Path(out) / "data").mkdir(parents=True, exist_ok=True)
    (Path(out) / "data/crew.json").write_text(json.dumps(crew, indent=1))
    print(len(crew), "crew")


if __name__ == "__main__":
    main(*sys.argv[1:3])
