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
    def run_gate(self, imported, test_status=0, args=(), changed=None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "tools").mkdir()
            (root / "game").mkdir()
            shutil.copyfile(ROOT / "tools/check.sh", root / "tools/check.sh")
            if (
                changed is not None
            ):  # a repository with one commit plus the given untracked files
                git = ["git", "-c", "user.name=t", "-c", "user.email=t@t"]
                subprocess.run([*git, "init", "-q"], cwd=root, check=True)
                subprocess.run([*git, "add", "."], cwd=root, check=True)
                subprocess.run([*git, "commit", "-qm", "base"], cwd=root, check=True)
                for name in changed:
                    (root / name).parent.mkdir(parents=True, exist_ok=True)
                    (root / name).write_text("")
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
            (root / "tools/shot.sh").write_text(shim)  # the startup smoke runs shot.sh
            (root / "tools/shot.sh").chmod(0o755)
            log = root / "calls"
            env = dict(
                os.environ,
                PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                GATE_LOG=str(log),
                IMPORTED="/fake/assets" if imported else "",
                TEST_STATUS=str(test_status),
                DISPLAY=":test",
            )
            result = subprocess.run(
                ["sh", str(root / "tools/check.sh"), *args],
                env=env,
                capture_output=True,
                text=True,
            )
            return result, log.read_text().splitlines()

    def last_line(self, result):
        return result.stdout.strip().splitlines()[-1]

    def test_default_tier_runs_the_tests_and_no_game(self):
        for imported in (False, True):
            with self.subTest(imported=imported):
                result, calls = self.run_gate(imported)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                tests = [
                    line for line in calls if line.split()[:2] == ["cargo", "test"]
                ]
                self.assertEqual(
                    tests,
                    ["cargo test --manifest-path game/Cargo.toml"
                     + (" -- --include-ignored" if imported else "")],
                )  # fmt: skip
                self.assertFalse(
                    [line for line in calls if line.startswith("timeout ")]
                )
                self.assertNotIn("python3 -m tools.battle_check", calls)
                self.assertIn("python3 -m unittest discover -s tests", calls)
                self.assertTrue(
                    self.last_line(result).startswith(
                        "GATE OK default" if imported else "GATE PARTIAL default"
                    )
                )

    def test_full_tier_runs_the_game(self):
        result, calls = self.run_gate(True, args=["--full"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(
            "cargo build --manifest-path game/Cargo.toml --profile fast", calls
        )
        # A self-quitting menu run with a picture, not a timed one.
        self.assertTrue(
            [
                line
                for line in calls
                if line.startswith("shot.sh") and "--menu main --shot-frame" in line
            ]
        )
        self.assertFalse([line for line in calls if line.startswith("timeout ")])
        self.assertIn("python3 -m tools.battle_check", calls)
        self.assertTrue(self.last_line(result).startswith("GATE OK full"))

    def test_full_tier_without_assets_fails_unless_partial_is_allowed(self):
        result, _ = self.run_gate(False, args=["--full"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("GATE PARTIAL full", result.stdout)
        result, calls = self.run_gate(False, args=["--full", "--allow-partial"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("python3 -m tools.battle_check", calls)
        self.assertIn("startup smoke, battle check", self.last_line(result))

    def test_rust_test_failure_stops_the_gate_after_the_cheap_checks(self):
        for imported in (False, True):
            with self.subTest(imported=imported):
                result, calls = self.run_gate(imported, test_status=1, args=["--full"])
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(
                    self.last_line(result).startswith("GATE FAIL rust-tests")
                )
                self.assertIn("rerun: cargo test", result.stdout)
                self.assertFalse(
                    [line for line in calls if line.startswith("cargo build")]
                )
                self.assertIn("python3 -m unittest discover -s tests", calls)

    def test_quick_tier_picks_steps_from_the_changed_paths(self):
        hygiene = "python3 -m unittest discover -s tests -p test_repo_hygiene.py"
        for changed, cargo, python_tests in (
            (["docs/notes.md"], False, False),
            (["tools/x.py"], False, True),
            (["game/src/x.rs"], True, False),
        ):
            with self.subTest(changed=changed):
                result, calls = self.run_gate(True, args=["--quick"], changed=changed)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(hygiene, calls)
                self.assertIn("ruff check .", calls)
                clippy = [line for line in calls if line.startswith("cargo clippy")]
                self.assertEqual(bool(clippy), cargo)
                self.assertFalse(
                    [line for line in calls if line.startswith("cargo test")]
                )
                self.assertEqual(
                    "python3 -m unittest discover -s tests" in calls, python_tests
                )
                self.assertRegex(
                    self.last_line(result), r"^GATE OK quick \d+s( \(.*\))?$"
                )
