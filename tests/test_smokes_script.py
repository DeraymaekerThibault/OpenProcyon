"""tools/smokes.sh runs each smoke in a child shell (xargs -> bash -c): the variables run_one uses must be exported to it."""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(
    shutil.which("bash") and shutil.which("xargs"), "needs bash, xargs"
)
@unittest.skipIf(
    sys.platform == "win32",
    "smokes.sh and the stand-ins are shebang scripts, which Windows cannot execute directly",
)
class SmokesScriptTests(unittest.TestCase):
    def test_one_smoke_runs_to_a_verdict_with_a_stub_game(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp).resolve()
            (tmp / "bin").mkdir()
            (tmp / "fast").mkdir()
            # Stands in for cargo (the build step) and for the game: writes the picture asked for with --shot.
            cargo = tmp / "bin/cargo"
            cargo.write_text("#!/bin/sh\nexit 0\n")
            game = tmp / "fast/openprocyon"
            game.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                "from PIL import Image\n"
                "Image.new('RGB', (8, 8), (200, 100, 50)).save(sys.argv[2], 'PNG')\n"
            )
            for f in (cargo, game):
                f.chmod(0o755)
            env = dict(
                os.environ,
                PATH=f"{tmp / 'bin'}{os.pathsep}{os.environ['PATH']}",
                CARGO_TARGET_DIR=str(tmp),
                PROCYON_ASSETS=str(tmp),
                DISPLAY=":fake",
            )
            env.pop("PROCYON_BIN", None)
            smokes = ROOT / "local/smokes"
            try:
                result = subprocess.run(
                    [str(ROOT / "tools/smokes.sh"), "--jobs", "1", "^mods$"],
                    env=env,
                    capture_output=True,
                    text=True,
                )
                log = result.stdout + result.stderr
                self.assertEqual(result.returncode, 0, log)
                self.assertIn("smokes: 1 ok, 0 FAIL", result.stdout)
                self.assertNotIn("command not found", log)
            finally:
                for f in smokes.glob("mods.*"):
                    f.unlink()
                shutil.rmtree(smokes / "overlay/mods", ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
