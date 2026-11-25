"""Pretty-prints the exported maps (also the per-map folders) and campaign data so they can be read, edited and diffed by hand (indent 2, number lists on one line).
usage: python3 -m tools.format_json <assets dir>"""

import json
from pathlib import Path

from tools import cli
from tools.jsonio import write


def main(assets):
    for pattern in ("maps/*.json", "campaign/*.json", "maps/*/*.json"):
        for f in sorted(Path(assets).glob(pattern)):
            write(f, json.loads(f.read_text(encoding="utf-8")))


if __name__ == "__main__":
    main(*cli.args(__doc__, 1))
