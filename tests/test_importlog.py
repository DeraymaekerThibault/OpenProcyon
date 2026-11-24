import tempfile
import unittest
from pathlib import Path

from tools import cli, export_docks, importlog, msb, wot


class ImportLogTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.log = self.root / "log" / "import.log"
        importlog.start(self.log)
        self.addCleanup(importlog.start)

    def test_summary_counts_and_file(self):
        importlog.skipped("a.wot", "bad")
        importlog.degraded("b.msb", "fallback")
        importlog.skipped("c.wot", "worse")
        self.assertEqual(
            importlog.summary(5), f"imported 5, degraded 1, skipped 2: see {self.log}"
        )
        self.assertEqual(
            self.log.read_text(encoding="utf-8").splitlines(),
            [
                "skipped: a.wot: bad",
                "degraded: b.msb: fallback",
                "skipped: c.wot: worse",
            ],
        )

    def test_msb_fallback_is_logged(self):
        self.assertEqual(msb.parse(b"not a scene", source="x.msb"), [])
        self.assertEqual(importlog.count(importlog.DEGRADED), 1)
        self.assertIn("x.msb", self.log.read_text(encoding="utf-8"))

    def test_msb_string_past_end_is_an_error(self):
        r = msb._Reader(b"\xff\x00\x00\x00abc")
        with self.assertRaises(ValueError):
            r.string()

    def test_broken_world_object_is_logged_as_skipped(self):
        wo = self.root / "game" / "WorldObjects"
        wo.mkdir(parents=True)
        (wo / "Ship_Bad.wot").write_bytes(b"{ garbage \xe9 \n")
        (self.root / "out").mkdir()
        export_docks.export(self.root / "game", self.root / "out")
        self.assertEqual(importlog.count(importlog.SKIPPED), 1)
        self.assertIn("Ship_Bad.wot", self.log.read_text(encoding="utf-8"))

    def test_wot_is_read_case_insensitively_as_latin1(self):
        wo = self.root / "WorldObjects"
        wo.mkdir()
        (wo / "SHIP_X.WOT").write_bytes(b"# caf\xe9\n")
        lines = wot.read_wot(self.root, "Ship_X")
        self.assertEqual(lines[0].text, "# café")

    def test_cli_wrong_arity_prints_usage(self):
        import sys

        argv, sys.argv = sys.argv, ["x", "only-one"]
        try:
            with self.assertRaises(SystemExit) as e:
                cli.args("Doc.\nusage: python -m x <a> <b>", 2)
        finally:
            sys.argv = argv
        self.assertEqual(e.exception.code, "usage: python -m x <a> <b>")


if __name__ == "__main__":
    unittest.main()
