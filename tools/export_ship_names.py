"""Export each ship type's pool of 'Available Unique Ship Name ID NN' (English) -> assets/data/ship_names.json {entity: [names]}.

Open skirmish maps name ships from these pools (historical maps set names in their world rules, see export_maps.py).
usage: python -m tools.export_ship_names <game_root> <assets_dir>"""

import json
import re
import sys
from pathlib import Path
from tools.export_maps import strings
from tools.mdb import ci_files


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
    root, assets = Path(sys.argv[1]), Path(sys.argv[2])
    p = pools(root, json.loads((assets / "ships.json").read_text()))
    (assets / "data" / "ship_names.json").write_text(json.dumps(p, indent=2) + "\n")
    print(f"{len(p)} ship types with name pools")
