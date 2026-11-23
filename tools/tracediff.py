"""Compare two PROCYON_TRACE runs (#535): one summary line per file pair, then the first row that differs.
usage: python3 -m tools.tracediff A.csv B.csv
A trace is `<name>.csv` (ships) plus `<name>.shells.csv` when the run fired; both pairs are compared. Exit status 1 when they differ.
Rows are compared as text: a fixed-step run with the same seed repeats itself exactly, so any change is a difference."""

import argparse
import itertools
import sys
from pathlib import Path


def compare(a, b):
    """(summary line, first differing rows or None) for two CSV files, read line by line."""
    with open(a, encoding="utf-8") as fa, open(b, encoding="utf-8") as fb:
        header = next(fa, "").rstrip("\n").split(",")
        next(fb, None)
        rows = differ = 0
        first = None
        for n, (ra, rb) in enumerate(itertools.zip_longest(fa, fb), start=2):
            rows += 1
            if ra != rb:
                differ += 1
                first = first or (n, ra, rb)
    name = Path(a).name
    if not differ:
        return f"same     {name}: {rows} rows", None
    n, ra, rb = first
    at = dict(zip(header, (ra or rb).split(","), strict=False))
    when = f"gt={at['gt']}" if "gt" in at else ""
    return (
        f"DIFFERENT {name}: {differ} of {rows} rows, first at line {n} {when}",
        (ra or "<missing>\n", rb or "<missing>\n"),
    )


def pairs(a, b):
    yield a, b
    sa, sb = Path(a).with_suffix(".shells.csv"), Path(b).with_suffix(".shells.csv")
    if sa.exists() or sb.exists():
        yield sa, sb


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("a")
    p.add_argument("b")
    args = p.parse_args(argv)
    same = True
    for a, b in pairs(args.a, args.b):
        if not (Path(a).exists() and Path(b).exists()):
            print(f"DIFFERENT {Path(a).name}: only one run wrote it")
            same = False
            continue
        line, first = compare(a, b)
        print(line)
        if first:
            same = False
            print(f"    A: {first[0]}", end="")
            print(f"    B: {first[1]}", end="")
    return 0 if same else 1


if __name__ == "__main__":
    sys.exit(main())
