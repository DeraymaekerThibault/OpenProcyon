"""Exercise the validation gate without invoking compilers or launching the game."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("sh"), "needs a POSIX shell")
class CheckGateTests(unittest.TestCase):
    def run_gate(self, imported, test_status=0):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "tools").mkdir()
            (root / "game").mkdir()
            shutil.copyfile(ROOT / "tools/check.sh", root / "tools/check.sh")
            bin_dir = root / "bin"
            bin_dir.mkdir()
            shim = """#!/bin/sh
name=${0##*/}
printf '%s\\n' "$name $*" >> "$GATE_LOG"
case "$name" in
  python3) [ "$1" != -c ] || echo "$IMPORTED" ;;
  cargo) [ "$1" != test ] || exit "$TEST_STATUS" ;;
  timeout) exit 124 ;;
esac
"""
            for name in ("ruff", "python3", "cargo", "timeout"):
                path = bin_dir / name
                path.write_text(shim)
                path.chmod(0o755)
            log = root / "calls"
            env = dict(
                os.environ,
                PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                GATE_LOG=str(log),
                IMPORTED=str(int(imported)),
                TEST_STATUS=str(test_status),
                DISPLAY=":test",
            )
            result = subprocess.run(
                ["sh", str(root / "tools/check.sh")],
                env=env,
                capture_output=True,
                text=True,
            )
            return result, log.read_text().splitlines()

    def test_rust_tests_run_in_both_modes(self):
        for imported in (False, True):
            with self.subTest(imported=imported):
                result, calls = self.run_gate(imported)
                self.assertEqual(result.returncode, 0, result.stderr)
                tests = [line for line in calls if line.startswith("cargo test ")]
                self.assertEqual(
                    tests,
                    [
                        "cargo test --release"
                        + (" -- --include-ignored" if imported else "")
                    ],
                )
                smokes = [line for line in calls if line.startswith("timeout ")]
                self.assertEqual(len(smokes), 1 if imported else 0)
                battles = [
                    line for line in calls if line == "python3 -m tools.battle_check"
                ]
                self.assertEqual(len(battles), 1 if imported else 0)
                self.assertIn("python3 -m unittest discover -s tests", calls)

    def test_rust_test_failure_stops_the_gate_in_both_modes(self):
        for imported in (False, True):
            with self.subTest(imported=imported):
                result, calls = self.run_gate(imported, test_status=1)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("cargo build --release", calls)
                self.assertNotIn("python3 -m unittest discover -s tests", calls)
