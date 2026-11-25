"""Export one mission-start campaign save per mission (assets/saves/missionNN.json) from the retail SavedGames/Mission NN.twt
snapshots: only the campaign counters carried into the mission (points, unlocked crew/arms). Mission 01 starts empty; mission 13
has no retail snapshot and reuses mission 12's counters (PROVISIONAL). See docs/re/scoring-saves.md.
"""

import re
from pathlib import Path

from tools import cli
from tools.gamefs import ci_files
from tools.jsonio import write


def counters(text):
    pts = re.search(r"^\s*SinglePlayerVictoryPoints Int (\d+)", text, re.MULTILINE)

    def lst(k):
        return re.findall(rf"{k}List - Element String '([^']*)'", text)

    return {
        "points": int(pts.group(1)) if pts else 0,
        "crew": lst("SinglePlayerCrew"),
        "arms": lst("SinglePlayerArms"),
    }


def main(root, out):
    root, out = Path(root), Path(out) / "saves"
    out.mkdir(parents=True, exist_ok=True)
    found = {}
    for n in range(2, 13):
        f = ci_files(root / "SavedGames", f"Mission {n:02}.twt")
        if f:
            found[n] = counters(f[0].read_text(encoding="latin1"))
    found[1] = {"points": 0, "crew": [], "arms": []}
    found[13] = found.get(
        12, found[max(found)]
    )  # campaign files may be missing or removed
    for n, c in sorted(found.items()):
        save = {"name": f"Mission {n:02} (start)", "mission": f"mission{n:02}", **c}
        write(out / f"mission{n:02}.json", save)
        print(save["name"], c["points"], len(c["crew"]), len(c["arms"]))


if __name__ == "__main__":
    main(*cli.args(__doc__, 2))
