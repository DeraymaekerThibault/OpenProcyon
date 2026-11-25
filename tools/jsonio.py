"""The one way the importer writes JSON: indent 2, number lists on one line, a final newline."""

import json
import re
from pathlib import Path

NUM = re.compile(r"\[\s*(-?[\d.eE+-]+(?:,\s*-?[\d.eE+-]+)*)\s*\]")


def dump(v, sort_keys=False):
    """indent=2, but number lists stay on one line."""
    return (
        NUM.sub(
            lambda m: "[" + re.sub(r"\s+", " ", m.group(1)) + "]",
            json.dumps(v, indent=2, sort_keys=sort_keys),
        )
        + "\n"
    )


def write(path, v, sort_keys=False):
    """Writes `v` to the file `path` as dump() text (UTF-8), creating its folders."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(dump(v, sort_keys), encoding="utf-8")
