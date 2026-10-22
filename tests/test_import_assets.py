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

    def test_steps_run_in_process_with_their_own_argv(self):
        # The frozen importer has no Python to start, so tool() runs `python -m <module>` in this process.
        with tempfile.TemporaryDirectory() as d:
            f = Path(d, "a.json")
            f.write_text('{"b":1}')
            argv = list(import_assets.sys.argv)
            self.assertEqual(import_assets.tool("tools.format_json", d), (True, ""))
            self.assertEqual(import_assets.sys.argv, argv)
            # A failing step stops the import, or with quiet reports its last error line.
            with (
                contextlib.redirect_stderr(io.StringIO()),
                self.assertRaises(import_assets.StepFailed),
            ):
                import_assets.tool("tools.format_json")
            ok, err = import_assets.tool("tools.format_json", quiet=True)
            self.assertFalse(ok)
            self.assertIn("IndexError", err)
            self.assertEqual(import_assets.sys.argv, argv)

    def test_frozen_default_out_is_the_folder_the_game_reads(self):
        # Same order as campaign::user_dir() in the game (its test covers the same cases).
        user_dir = import_assets.user_dir
        with tempfile.TemporaryDirectory() as d:
            env = {"HOME": str(Path(d, "h")), "APPDATA": str(Path(d, "a"))}
            home, appdata = (
                Path(d, "h/.local/share/openprocyon"),
                Path(d, "a/openprocyon"),
            )
            self.assertEqual(
                user_dir({**env, "XDG_DATA_HOME": "/x"}, True), Path("/x/openprocyon")
            )
            self.assertEqual(user_dir(env, windows=False), home)
            self.assertEqual(user_dir(env, windows=True), appdata)
            # A HOME folder made earlier (Git Bash sets HOME) stays in use until an APPDATA one exists.
            home.mkdir(parents=True)
            self.assertEqual(user_dir(env, windows=True), home)
            appdata.mkdir(parents=True)
            self.assertEqual(user_dir(env, windows=True), appdata)
            self.assertEqual(user_dir({"HOME": env["HOME"]}, windows=True), home)
            self.assertIsNone(user_dir({}, windows=True))


if __name__ == "__main__":
    unittest.main()
