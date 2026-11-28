import io
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import rungame

NEW_XVFB = "usage: xvfb-run [options] command\n-d, --auto-display\n"
LVP = [Path("/usr/share/vulkan/icd.d/lvp_icd.x86_64.json")]


def plan(environ, platform="linux", jobs=1, xvfb_help=NEW_XVFB, icds=LVP):
    return rungame.launch_plan(environ, platform, jobs, xvfb_help, icds)


class RunMode(unittest.TestCase):
    def test_auto_keeps_the_old_detection(self):
        self.assertEqual(rungame.run_mode({"DISPLAY": ":0"}, "linux"), "window")
        self.assertEqual(
            rungame.run_mode({"WAYLAND_DISPLAY": "wayland-0"}, "linux"), "window"
        )
        self.assertEqual(rungame.run_mode({}, "linux"), "software")
        self.assertEqual(rungame.run_mode({"PROCYON_RUN": "auto"}, "linux"), "software")
        self.assertEqual(
            rungame.run_mode({"PROCYON_RUN": ""}, "linux"), "software", "empty is unset"
        )
        self.assertEqual(rungame.run_mode({}, "win32"), "window")
        self.assertEqual(rungame.run_mode({}, "msys"), "window")

    def test_explicit_modes_win_over_the_display(self):
        self.assertEqual(
            rungame.run_mode({"PROCYON_RUN": "software", "DISPLAY": ":0"}, "linux"),
            "software",
        )
        self.assertEqual(rungame.run_mode({"PROCYON_RUN": "window"}, "linux"), "window")

    def test_an_unknown_mode_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "not a run mode"):
            rungame.run_mode({"PROCYON_RUN": "gpu"}, "linux")
        with self.assertRaisesRegex(ValueError, "not a run mode"):
            rungame.run_mode({"PROCYON_RUN": "gpu-hidden"}, "linux")


class LaunchPlan(unittest.TestCase):
    def test_window_runs_on_x11_without_a_prefix(self):
        p = plan({"DISPLAY": ":0", "WAYLAND_DISPLAY": "wayland-0"})
        self.assertEqual((p.mode, p.prefix), ("window", []))
        self.assertEqual(
            p.env, {"WAYLAND_DISPLAY": ""}, "never native Wayland, nothing else changed"
        )

    def test_window_on_windows_changes_nothing(self):
        self.assertEqual(plan({}, "win32"), rungame.Launch("window", [], {}))

    def test_a_window_needs_an_x11_display(self):
        with self.assertRaisesRegex(ValueError, "DISPLAY"):
            plan({"WAYLAND_DISPLAY": "wayland-0"})
        with self.assertRaisesRegex(ValueError, "DISPLAY"):
            plan({"PROCYON_RUN": "window"})

    def test_software_is_xvfb_and_mesa_vulkan(self):
        p = plan({"PROCYON_RUN": "software", "DISPLAY": ":0"})
        self.assertEqual(p.prefix, ["xvfb-run", "-d", "-s", "-screen 0 1280x720x24"])
        self.assertEqual(
            p.env,
            {
                "WAYLAND_DISPLAY": "",
                "WGPU_BACKEND": "vulkan",
                "VK_ICD_FILENAMES": str(LVP[0]),
            },
        )
        old = plan({}, xvfb_help="usage: xvfb-run [-a]", icds=[])
        self.assertEqual(old.prefix[1], "-a", "older xvfb-run has only -a")
        self.assertNotIn(
            "VK_ICD_FILENAMES", old.env, "no lavapipe installed: Vulkan picks"
        )
        with self.assertRaisesRegex(ValueError, "Windows"):
            plan({"PROCYON_RUN": "software"}, "win32")

    def test_parallel_software_runs_share_the_cores(self):
        self.assertNotIn(
            "LP_NUM_THREADS", plan({}).env, "one run: llvmpipe's default, every core"
        )
        threads = int(plan({}, jobs=4).env["LP_NUM_THREADS"])
        self.assertEqual(threads, rungame.software_threads(4))
        self.assertEqual(rungame.software_threads(4, cores=16), 4)
        self.assertEqual(rungame.software_threads(32, cores=16), 1)
        self.assertNotIn(
            "LP_NUM_THREADS",
            plan({"LP_NUM_THREADS": "3"}, jobs=4).env,
            "the caller's value wins",
        )
        self.assertNotIn(
            "LP_NUM_THREADS",
            plan({"DISPLAY": ":0"}, jobs=4).env,
            "a GPU run has no llvmpipe",
        )


class GameEnv(unittest.TestCase):
    def test_extra_wins_over_the_launch_settings(self):
        launch = rungame.Launch(
            "software", ["xvfb-run"], {"WGPU_BACKEND": "vulkan", "WAYLAND_DISPLAY": ""}
        )
        env = rungame.game_env(
            "/x", launch, PROCYON_OVERLAY="/o", A="1", WGPU_BACKEND="gl"
        )
        self.assertEqual(env["XDG_DATA_HOME"], "/x")
        self.assertEqual(env["WAYLAND_DISPLAY"], "")
        self.assertEqual(env["PROCYON_OVERLAY"], "/o")
        self.assertEqual(env["A"], "1")
        self.assertEqual(env["WGPU_BACKEND"], "gl")

    def test_every_tool_run_checks_the_tick_order(self):
        env = rungame.game_env("/x", rungame.Launch("window", [], {}))
        self.assertEqual(env["PROCYON_SCHEDULE_CHECK"], "1")


class Summary(unittest.TestCase):
    LOG = (
        'INFO bevy_render::renderer: AdapterInfo { name: "llvmpipe (LLVM 19.1.7, 256 bits)", vendor: 65541, device: 0, '
        'device_type: Cpu, device_pci_bus_id: "", driver: "llvmpipe", driver_info: "Mesa 25.0.7", backend: Vulkan, '
        "transient_saves_memory: false }\n"
        "INFO openprocyon::exit: run end: frame 3120\n"
    )

    def test_names_mode_adapter_backend_and_frame_rate(self):
        line = rungame.summary("software", self.LOG, 52.0)
        self.assertEqual(
            line,
            'run: mode=software adapter "llvmpipe (LLVM 19.1.7, 256 bits)" (Cpu, Vulkan) 3120 frames in 52.0 s (60.0 fps)',
        )

    def test_says_what_it_could_not_find(self):
        line = rungame.summary("window", "", 3.0)
        self.assertIn("mode=window", line)
        self.assertIn("adapter unknown", line)
        self.assertIn("frame count unknown", line)


class Run(unittest.TestCase):
    def test_passes_output_through_and_prints_the_run_line(self):
        game = [
            sys.executable,
            "-c",
            "print('INFO x: run end: frame 7'); raise SystemExit(4)",
        ]
        out = io.BytesIO()
        wrapper = io.TextIOWrapper(out, write_through=True)
        with mock.patch.object(sys, "stdout", wrapper):
            code = rungame.run(
                game, {**os.environ, "PROCYON_RUN": "window", "DISPLAY": ":0"}
            )
        text = out.getvalue().decode()
        wrapper.detach()
        self.assertEqual(code, 4)
        self.assertIn("run end: frame 7", text)
        self.assertRegex(text, r"run: mode=window adapter unknown .* 7 frames in")

    def test_the_command_line_rejects_an_unknown_mode(self):
        with (
            mock.patch.dict(os.environ, {"PROCYON_RUN": "fast"}),
            mock.patch.object(sys, "stderr", io.StringIO()) as err,
        ):
            self.assertEqual(rungame.main(["mode"]), 2)
        self.assertIn("not a run mode", err.getvalue())


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
