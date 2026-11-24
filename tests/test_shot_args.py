"""tools/shot.sh runs the game from game/: relative folder arguments must be made absolute against the caller's directory first."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class ShotArgumentTests(unittest.TestCase):
    def run_shot(self, *args):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp).resolve()
            record = tmp / "args.txt"
            fake = tmp / "fake-game"
            # Stands in for the game: records its arguments and its working directory, writes the picture asked for with --shot.
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import os, sys\n"
                "from PIL import Image\n"
                f"open({str(record)!r}, 'w').write('\\n'.join(sys.argv[1:]) + '\\n' + os.getcwd())\n"
                "Image.new('RGB', (8, 8), (200, 100, 50)).save(sys.argv[2], 'PNG')\n"
            )
            fake.chmod(0o755)
            (tmp / "work").mkdir()
            env = dict(
                os.environ,
                PROCYON_BIN=str(fake),
                PROCYON_ASSETS=str(tmp),
                DISPLAY=":fake",
            )
            result = subprocess.run(
                [str(ROOT / "tools/shot.sh"), "out.png", *args],
                cwd=tmp / "work",
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = record.read_text().split("\n")
            return tmp, lines[:-1], Path(lines[-1])

    def test_relative_mod_paths_resolve_against_the_callers_directory(self):
        tmp, got, cwd = self.run_shot(
            "--mod", "../m1", "--shot-frame", "3", "--mod", "sub/m2", "--mod", "/abs/m3"
        )
        self.assertEqual(cwd, ROOT / "game")
        self.assertEqual(got[0], "--shot")
        self.assertEqual(
            got[2:],
            [
                "--mod",
                str(tmp / "m1"),
                "--shot-frame",
                "3",
                "--mod",
                str(tmp / "work/sub/m2"),
                "--mod",
                "/abs/m3",
            ],
        )

    def test_other_arguments_pass_unchanged(self):
        _, got, _ = self.run_shot(
            "--menu", "main", "--shot-frame", "1", "--script", "fxtest"
        )
        self.assertEqual(
            got[2:], ["--menu", "main", "--shot-frame", "1", "--script", "fxtest"]
        )


if __name__ == "__main__":
    unittest.main()
