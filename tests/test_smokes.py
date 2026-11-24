"""tools/smokes.py: the change -> smoke map and the verdict of a run (no game is launched)."""

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools import gamelog, smokes

REPO = Path(__file__).resolve().parents[1]


def table():
    """(name, kind) of every row in tools/smokes.sh."""
    text = (REPO / "tools/smokes.sh").read_text()
    rows = re.search(r"<<'EOF2'\n(.*?)\nEOF2", text, re.DOTALL)[1].splitlines()
    return [tuple(r.split("|")[:2]) for r in rows]


class PlanTests(unittest.TestCase):
    def test_combat_runs_battles_and_the_combat_smokes(self):
        names, unmapped = smokes.plan(["game/src/combat/shells.rs"])
        self.assertEqual(names, ["battles", "damage", "target", "course"])
        self.assertEqual(unmapped, [])

    def test_rows_add_up_without_repeats(self):
        names, _ = smokes.plan(
            ["game/src/docking.rs", "game/src/boarding.rs", "game/src/ship.rs"]
        )
        self.assertEqual(names, ["dock", "battles", "damage", "target", "course"])

    def test_non_game_files_and_unknown_game_files(self):
        self.assertEqual(smokes.plan(["docs/x.md", "tools/foo.py"]), ([], []))
        self.assertEqual(
            smokes.plan(["game/src/audio.rs"]), ([], ["game/src/audio.rs"])
        )

    def test_every_mapped_name_is_a_smoke(self):
        known = {n for n, _ in table()} | {"battles", "all"}
        for _, names in smokes.MAP:
            self.assertLessEqual(set(names), known)

    def test_table_kinds(self):
        self.assertTrue(table())
        self.assertLessEqual({k for _, k in table()}, {"asserts", "visual"})


class VerdictTests(unittest.TestCase):
    def test_asserting_script_needs_its_pass_line(self):
        self.assertIsNone(
            smokes.verdict("asserts", 0, "x\nsmoke::lifeboats: lifeboat-m1: PASS\n")
        )
        self.assertIsNone(
            smokes.verdict("asserts", 1, "lifeboat-m1: PASS")
        )  # quit before its picture
        self.assertIsNone(
            smokes.verdict("asserts", 0, "fleet ECS/UI smoke passed: selection")
        )
        self.assertIn("PASS line", smokes.verdict("asserts", 0, "shot.sh: saved x.png"))
        self.assertEqual(
            smokes.verdict("asserts", 0, "PASS\nthread 'main' panicked at x"),
            "panicked",
        )

    def test_visual_script_needs_a_clean_exit(self):
        self.assertIsNone(smokes.verdict("visual", 0, ""))
        self.assertIn("3", smokes.verdict("visual", 3, ""))

    def test_tail_prefers_relevant_lines(self):
        log = (
            "\n".join(f"noise {i}" for i in range(30))
            + "\nthread panicked at a\nmore\n"
        )
        self.assertEqual(smokes.tail(log), ["thread panicked at a"])
        self.assertEqual(len(smokes.tail("\n".join(f"n{i}" for i in range(30)))), 10)


class FloodTests(unittest.TestCase):
    """#561: a flooded log is read as its markers plus a bounded tail, never whole."""

    def test_markers_survive_and_the_flood_is_cut(self):
        flood = (
            "\x1b[31mERROR\x1b[0m rodio::stream: audio stream error: POLLERR\n" * 50_000
        )
        # newline="": no \r\n translation on Windows, the log is byte-exact like the game's own
        with tempfile.NamedTemporaryFile(
            "w", suffix=".log", delete=False, newline=""
        ) as f:
            f.write("fleet ECS/UI smoke passed: selection\n" + flood + "last line\n")
        text = gamelog.read(f.name, "panicked at|smoke passed", tail=1000)
        self.assertLess(len(text), 1200)
        self.assertTrue(text.startswith("fleet ECS/UI smoke passed"))
        self.assertTrue(text.endswith("last line\n"))
        self.assertNotIn("\x1b", text)
        verdict = subprocess.run(
            [sys.executable, "-m", "tools.smokes", "verdict", "asserts", "0", f.name],
            cwd=REPO,
            capture_output=True,
            text=True,
        )
        Path(f.name).unlink()
        self.assertEqual(verdict.returncode, 0, verdict.stdout)


if __name__ == "__main__":
    unittest.main()
