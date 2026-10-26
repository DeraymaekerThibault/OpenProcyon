"""Read-only, provenance-preserving text ingestion; NOT a WOT format parser."""

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class SourceLine:
    path: (
        str  # Canonical path relative to the supplied input root, never the local root.
    )
    line: int  # One-based physical line number.
    text: str  # Whitespace preserved except for the line ending.


def read_source_lines(
    root: Path, relative_path: Path, *, encoding: str
) -> list[SourceLine]:
    """Read one explicitly selected text file without scanning or writing.

    Encoding is mandatory: the original files' encoding is not yet known.
    Decoding errors propagate rather than silently replacing source characters.
    LF, CRLF and CR are normalized; blank lines and duplicate text are retained.
    Absolute paths and paths/symlinks escaping the input root are rejected.
    This is for trusted local inputs, not a concurrent hostile filesystem.
    """
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise NotADirectoryError(root)
    if relative_path.is_absolute():
        raise ValueError("Select a file using a path relative to the input root")
    source = (root / relative_path).resolve(strict=True)
    if not source.is_relative_to(root):
        raise ValueError("Selected file resolves outside the input root")
    if not source.is_file():
        raise ValueError("Selected input must be a regular file")

    provenance = source.relative_to(root).as_posix()
    with source.open("r", encoding=encoding, errors="strict", newline=None) as stream:
        return [
            SourceLine(provenance, number, text.removesuffix("\n"))
            for number, text in enumerate(stream, start=1)
        ]
