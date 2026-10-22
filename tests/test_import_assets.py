import contextlib
import io
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import import_assets


class ImportAssets(unittest.TestCase):
    def test_read_env_takes_quoted_values_and_skips_comments(self):
        with tempfile.TemporaryDirectory() as d:
            env = Path(d, ".env")
            env.write_text(
                '# install\nPROCYON_GAME_ROOT="D:/Games/My Game"\nOTHER=x\nnoise\n'
            )
            self.assertEqual(
                import_assets.read_env(env),
                {"PROCYON_GAME_ROOT": "D:/Games/My Game", "OTHER": "x"},
            )
            self.assertEqual(import_assets.read_env(Path(d, "missing")), {})

    def test_refuses_a_folder_that_is_not_an_install(self):
        with tempfile.TemporaryDirectory() as d, self.assertRaises(SystemExit) as e:
            import_assets.main(["--game", d, "--out", str(Path(d, "out"))])
        self.assertIn("not a game install", str(e.exception.code))
        self.assertFalse(Path(d, "out").exists())

    def test_numbers_steps_and_stops_at_the_first_failure(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "game/WorldObjects").mkdir(parents=True)
            (Path(d) / "game/WorldObjects/Ship_Extra.wot").write_text("")
            ran = []

            def fail():
                raise import_assets.StepFailed("tools.export_hud")

            def fake_steps(root, out, want):
                ran.append(want)
                return [("one", lambda: None), ("two", fail), ("three", ran.clear)]

            printed = io.StringIO()
            with (
                mock.patch.object(import_assets, "steps", fake_steps),
                contextlib.redirect_stdout(printed),
                self.assertRaises(SystemExit) as e,
            ):
                import_assets.main(
                    ["--game", str(Path(d, "game")), "--out", str(Path(d, "out"))]
                )
            self.assertEqual(
                printed.getvalue().splitlines(), ["[1/3] one", "[2/3] two"]
            )
            self.assertIn("'two'", str(e.exception.code))
            # The curated skirmish ships come first, then every other hull in the install.
            self.assertEqual(ran, [[*import_assets.FIRST, "Ship_Extra"]])


if __name__ == "__main__":
    unittest.main()
