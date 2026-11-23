"""The one way the importer writes JSON: indent 2, number lists on one line, a final newline."""

import json
import re

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
