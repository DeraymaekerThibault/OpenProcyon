"""tools/worktree.sh new/rm/gc on a scratch repository (no compiler: the copied build is a few fake files)."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("bash") and shutil.which("git"), "needs bash and git")
class WorktreeTool(unittest.TestCase):
    def test_new_copies_the_build_without_incremental_and_rm_removes_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            main = Path(tmp, "main")
            (main / "tools").mkdir(parents=True)
            shutil.copy(ROOT / "tools/worktree.sh", main / "tools")
            env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t"}
            env |= {"GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}

            def sh(*args, cwd=main):
                return subprocess.run(
                    args, cwd=cwd, env=env, check=True, capture_output=True, text=True
                ).stdout

            sh("git", "init", "-q", "-b", "main")
            (main / ".gitignore").write_text("game/target/\n.venv\n")
            sh("git", "add", ".")
            sh("git", "commit", "-qm", "base")
            sh(
                "git", "branch", "topic"
            )  # an existing branch: no fetch of origin needed
            (main / "game/target/debug/deps").mkdir(parents=True)
            (main / "game/target/debug/deps/libx.rlib").write_text("x")
            (main / "game/target/debug/incremental/y").mkdir(parents=True)
            (main / ".venv").mkdir()
            wt = Path(tmp, "wt")
            sh("tools/worktree.sh", "new", "topic", str(wt))
            self.assertTrue((wt / "game/target/debug/deps/libx.rlib").is_file())
            self.assertFalse((wt / "game/target/debug/incremental").exists())
            self.assertTrue((wt / ".venv").is_symlink())
            self.assertIn("git -C", sh("tools/worktree.sh", "gc", "-n"))
            sh("tools/worktree.sh", "rm", str(wt))
            self.assertFalse(wt.exists())
            self.assertNotIn(str(wt), sh("git", "worktree", "list"))
