import struct
import tempfile
import unittest
from pathlib import Path

from tools.ogg import duration


def page(granule, payload, serial=1, sequence=0):
    # CRC is not checked by the reader; one lacing segment per page is enough for small payloads.
    return (
        struct.pack("<4sBBqIIIB", b"OggS", 0, 0, granule, serial, sequence, 0, 1)
        + bytes([len(payload)])
        + payload
    )


def ident(rate):
    return b"\x01vorbis" + struct.pack("<IBI", 0, 2, rate) + bytes(15)


class Ogg(unittest.TestCase):
    def write(self, data):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        path = Path(d.name, "a.ogg")
        path.write_bytes(data)
        return path

    def test_duration_is_last_granule_over_rate(self):
        data = page(0, ident(22050)) + page(4410, b"x" * 9, sequence=1)
        self.assertEqual(
            duration(self.write(data + page(33075, b"y", sequence=2))), 1.5
        )

    def test_rounds_to_microseconds_like_ffprobe(self):
        data = page(0, ident(44100)) + page(100, b"x", sequence=1)
        self.assertEqual(duration(self.write(data)), 0.002268)

    def test_skips_pages_without_granule_and_other_streams(self):
        data = page(0, ident(1000)) + page(2500, b"x", sequence=1)
        data += page(-1, b"y", sequence=2) + page(9999, b"z", serial=2)
        self.assertEqual(duration(self.write(data)), 2.5)

    def test_rejects_non_vorbis(self):
        with self.assertRaises(ValueError):
            duration(self.write(page(0, b"\x7fFLAC" + bytes(30))))


if __name__ == "__main__":
    unittest.main()
