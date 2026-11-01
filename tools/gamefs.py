"""Read-only file lookup inside a retail install. The original game ran on Windows, so names in its data files (and the
files on a case-sensitive copy of the install) do not reliably match case."""

import fnmatch
from pathlib import Path


def ci_files(directory, pattern):
    """Case-insensitive glob in one directory, sorted by path; empty when the directory does not exist."""
    directory = Path(directory)
    if not directory.is_dir():
        return []
    return sorted(
        p
        for p in directory.iterdir()
        if fnmatch.fnmatch(p.name.lower(), pattern.lower())
    )


def ci_file(directory, name):
    """The first case-insensitive match of `name` in `directory`; FileNotFoundError when there is none."""
    found = ci_files(directory, name)
    if not found:
        raise FileNotFoundError(f"{name} not found in {directory}")
    return found[0]
