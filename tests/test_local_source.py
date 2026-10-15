"""Opt-in text/provenance smoke test; never asserts WOT semantics or ship counts."""

import hashlib
import os
import unittest
from pathlib import Path

from tools.source_text import read_source_lines


@unittest.skipUnless(
    os.environ.get("PROCYON_RUN_LOCAL_TESTS") == "1",
    "local input check is opt-in (PROCYON_RUN_LOCAL_TESTS=1)",
)
class LocalSourceTests(unittest.TestCase):
    def test_explicit_local_text_file(self):
        names = ("PROCYON_GAME_ROOT", "PROCYON_SOURCE_FILE", "PROCYON_SOURCE_ENCODING")
        for name in names:
            self.assertTrue(os.environ.get(name), f"Opt-in test requires {name}")
        root = Path(os.environ["PROCYON_GAME_ROOT"]).resolve(strict=True)
        relative = Path(os.environ["PROCYON_SOURCE_FILE"])
        encoding = os.environ["PROCYON_SOURCE_ENCODING"]
        workspace = Path(__file__).resolve().parents[1]
        self.assertFalse(
            root.is_relative_to(workspace), "Use an input root outside the workspace"
        )
        self.assertFalse(
            relative.is_absolute(), "PROCYON_SOURCE_FILE must be root-relative"
        )
        source = (root / relative).resolve(strict=True)
        self.assertTrue(source.is_relative_to(root), "Input must stay within its root")
        self.assertFalse(
            source.is_relative_to(workspace), "Use a file outside the workspace"
        )
        before = source.read_bytes()
        records = read_source_lines(root, relative, encoding=encoding)
        expected = (
            before.decode(encoding, errors="strict")
            .replace("\r\n", "\n")
            .replace("\r", "\n")
            .split("\n")
        )
        if expected[-1] == "":
            expected.pop()
        # Avoid dumping original text into test logs on failure.
        self.assertTrue(
            [record.text for record in records] == expected, "Source text mismatch"
        )
        self.assertEqual(
            [record.line for record in records], list(range(1, len(expected) + 1))
        )
        self.assertTrue(
            all(
                record.path == source.relative_to(root).as_posix() for record in records
            )
        )
        self.assertEqual(
            hashlib.sha256(source.read_bytes()).digest(),
            hashlib.sha256(before).digest(),
        )


if __name__ == "__main__":
    unittest.main()
