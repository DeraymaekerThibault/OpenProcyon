"""Read a game log without loading it whole (#561: a flooded log once grew a reader to 19 GB).
`read(path, keep)` streams the file and returns the lines matching `keep` plus the last `tail` bytes, ANSI colours removed."""

import os
import re

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TAIL = 4 << 20


def read(path, keep=r"panicked at", tail=TAIL):
    keep = re.compile(keep)
    with open(path, "rb") as f:
        kept = [
            ln.decode("utf-8", "replace")
            for ln in f
            if keep.search(ANSI.sub("", ln.decode("utf-8", "replace")))
        ]
        f.seek(max(0, f.seek(0, os.SEEK_END) - tail))
        end = f.read().decode("utf-8", "replace")
    return ANSI.sub("", "".join(kept) + end)
