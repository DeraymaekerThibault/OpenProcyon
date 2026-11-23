"""Export each ship type's pool of 'Available Unique Ship Name ID NN' (English) -> assets/data/ship_names.json {entity: [names]}.

Open skirmish maps name ships from these pools (historical maps set names in their world rules, see export_maps.py).
usage: python -m tools.export_ship_names <game_root> <assets_dir>"""

import json
import re
from pathlib import Path

from tools import cli
from tools.export_maps import strings
from tools.gamefs import ci_files


def pools(root, entities):
    text = strings(root)
    out = {}
    for e in entities:
        for f in ci_files(Path(root) / "WorldObjects", f"{e}.wot"):
            ids = re.findall(
                r"Available Unique Ship Name ID \d+ String '([^']*)'",
                f.read_text(encoding="latin1"),
            )
            names = [text[i] for i in ids if i in text]
            if names:
                out[e] = names
    return out


if __name__ == "__main__":
    root, assets = map(Path, cli.args(__doc__, 2))
    p = pools(root, json.loads((assets / "ships.json").read_text(encoding="utf-8")))
    (assets / "data" / "ship_names.json").write_text(
        json.dumps(p, indent=2) + "\n", encoding="utf-8"
    )
    print(f"{len(p)} ship types with name pools")
