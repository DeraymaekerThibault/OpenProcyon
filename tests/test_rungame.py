import sys
import tempfile
import unittest
from pathlib import Path

from tools import rungame


class GameEnv(unittest.TestCase):
    def test_extra_wins_and_a_virtual_display_selects_vulkan(self):
        env = rungame.game_env("/x", ["xvfb-run"], PROCYON_OVERLAY="/o", A="1")
        self.assertEqual(env["XDG_DATA_HOME"], "/x")
        self.assertEqual(env["WAYLAND_DISPLAY"], "")
        self.assertEqual(env["PROCYON_OVERLAY"], "/o")
        self.assertEqual(env["A"], "1")
        self.assertEqual(env["WGPU_BACKEND"], "vulkan")
        self.assertNotEqual(rungame.game_env("/x", []).get("WGPU_BACKEND"), "vulkan")


class RunLogged(unittest.TestCase):
    def test_logs_output_and_reports_exit_code_or_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "run.log"
            code = rungame.run_logged(
                [sys.executable, "-c", "import sys; print('hi'); sys.exit(3)"],
                log,
                {},
                30,
            )
            self.assertEqual(code, 3)
            self.assertIn("hi", log.read_text())
            hung = [sys.executable, "-c", "import time; time.sleep(30)"]
            self.assertIsNone(rungame.run_logged(hung, log, {}, 0.5))


if __name__ == "__main__":
    unittest.main()
