"""Argument handling shared by the exporter command lines: a wrong argument count prints the module's usage line."""

import sys


def args(doc, lo, hi=None):
    """sys.argv[1:] when it has between `lo` and `hi` (default: exactly `lo`) entries; else exits with the `usage:` line of `doc`."""
    given = sys.argv[1:]
    if not lo <= len(given) <= (lo if hi is None else hi):
        usage = [
            ln[ln.index("usage:") :]
            for ln in (doc or "").splitlines()
            if "usage:" in ln
        ]
        sys.exit("\n".join(usage) or f"expected {lo} arguments, got {len(given)}")
    return given
