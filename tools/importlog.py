"""The import log: every fallback ("degraded": the data was read by a weaker method) and every skip (the file or object is
missing from the output) of an import is recorded with its source and reason. The importer prints the totals at the end
(`summary()`) and points at the log file; run on their own, the exporters print each entry to stderr."""

import sys

DEGRADED, SKIPPED = "degraded", "skipped"
_events = []
_path = None


def start(path=None):
    """Begins a new import: forgets earlier entries; with a path, entries are also written there (the file is replaced)."""
    global _path
    _events.clear()
    _path = path
    if path:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("", encoding="utf-8")


def _note(kind, source, reason):
    line = f"{kind}: {source}: {reason}"
    _events.append((kind, str(source), str(reason)))
    if _path:
        with _path.open("a", encoding="utf-8") as f:
            f.write(line + "\n")
    else:
        print(line, file=sys.stderr)


def degraded(source, reason):
    _note(DEGRADED, source, reason)


def skipped(source, reason):
    _note(SKIPPED, source, reason)


def count(kind):
    return sum(1 for k, _, _ in _events if k == kind)


def summary(imported):
    """`imported N, degraded M, skipped K` (+ where the details are, when a log file is in use)."""
    line = f"imported {imported}, degraded {count(DEGRADED)}, skipped {count(SKIPPED)}"
    return f"{line}: see {_path}" if _path else line
