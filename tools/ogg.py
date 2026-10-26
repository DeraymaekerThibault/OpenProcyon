"""Ogg Vorbis duration without external tools: the sample count is the granule position of the last page, the rate comes from
the Vorbis identification header in the first page. Matches `ffprobe -show_entries format=duration` to the microsecond.
usage: python -m tools.ogg <file.ogg> ..."""

import struct
import sys
from pathlib import Path

# Page header: capture, version, flags, granule, serial, sequence, crc, segment count.
HEADER = struct.Struct("<4sBBqIIIB")


def _page(data, at):
    """(granule, serial, payload start, payload end) of the page at `at`."""
    capture, _, _, granule, serial, _, _, count = HEADER.unpack_from(data, at)
    if capture != b"OggS":
        raise ValueError(f"no Ogg page at {at}")
    start = at + HEADER.size + count
    return granule, serial, start, start + sum(data[at + HEADER.size : start])


def duration(path):
    """Length in seconds, rounded to microseconds like ffprobe."""
    data = Path(path).read_bytes()
    _, serial, start, _ = _page(data, 0)
    if data[start : start + 7] != b"\x01vorbis":
        raise ValueError(f"{path}: not Ogg Vorbis")
    rate = struct.unpack_from("<I", data, start + 12)[0]
    # The last page that ends a packet in the Vorbis stream carries the final sample count (-1 means no packet ends there).
    at = data.rfind(b"OggS")
    while at >= 0:
        granule, page_serial, _, end = _page(data, at)
        if page_serial == serial and granule >= 0 and end <= len(data):
            return round(granule / rate, 6)
        at = data.rfind(b"OggS", 0, at)
    raise ValueError(f"{path}: no granule position")


if __name__ == "__main__":
    for f in sys.argv[1:]:
        print(f"{duration(f):.6f} {f}")
