"""Pretty-prints the exported maps (also the per-map folders) and campaign data so they can be read, edited and diffed by hand (indent 2, number lists on one line).
usage: python3 tools/format_json.py <assets dir>"""

import json
import sys
from pathlib import Path

from tools.pack_data import dump

for pattern in ("maps/*.json", "campaign/*.json", "maps/*/*.json"):
    for f in sorted(Path(sys.argv[1]).glob(pattern)):
        f.write_text(dump(json.loads(f.read_text())))
