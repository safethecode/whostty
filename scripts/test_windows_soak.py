import importlib.util
import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("windows-soak.py")


def load_soak():
    spec = importlib.util.spec_from_file_location("windows_soak", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class WindowsSoakTest(unittest.TestCase):
    def child_command(self, directory, *extra):
        return [sys.executable, str(SCRIPT), "--child", "--output", str(directory),
                "--token", "unit-test", *extra]

    def test_fixture_emits_paced_unicode_and_ansi_with_exact_counters(self):
        with tempfile.TemporaryDirectory() as temporary:
            started = time.monotonic()
            result = subprocess.run(self.child_command(
                temporary, "--duration", "0.4", "--lines-per-second", "100",
                "--alternate-interval", "0.1"), capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            status = json.loads((Path(temporary) / "child-status.json").read_text(encoding="utf-8"))
            self.assertEqual(status["state"], "completed")
            self.assertEqual(status["reason"], "duration")
            self.assertEqual(status["token"], "unit-test")
            self.assertEqual(status["bytes"], len(result.stdout))
            self.assertEqual(status["lines"], result.stdout.count(b"\n"))
            self.assertGreaterEqual(status["lines"], 20)
            self.assertLessEqual(status["lines"], 45)
            self.assertIn("한글".encode(), result.stdout)
            self.assertIn(b"\x1b[?1049h", result.stdout)
            self.assertIn(b"\x1b[?1049l", result.stdout)
            self.assertGreaterEqual(time.monotonic() - started, 0.35)

    def test_stop_file_ends_long_fixture_gracefully(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            with subprocess.Popen(self.child_command(
                    directory, "--duration", "20", "--lines-per-second", "100"),
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
                deadline = time.monotonic() + 3
                while not (directory / "child-status.json").exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                (directory / "stop").touch()
                output, error = process.communicate(timeout=3)
                self.assertEqual(process.returncode, 0, error.decode(errors="replace"))
            status = json.loads((directory / "child-status.json").read_text(encoding="utf-8"))
            self.assertEqual(status["reason"], "stop_requested")
            self.assertEqual(status["state"], "completed")
            self.assertEqual(status["bytes"], len(output))
            self.assertLess(status["elapsed_seconds"], 3)

    def test_arguments_reject_nonfinite_and_unbounded_work(self):
        soak = load_soak()
        for option, value in [("--duration", "nan"), ("--duration", "inf"),
                              ("--duration", "0"), ("--lines-per-second", "1000000"),
                              ("--sample-seconds", "0"), ("--alternate-interval", "-1")]:
            with self.subTest(option=option, value=value), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                soak.parse_args(["--child", "--output", "unused", "--token", "test", option, value])

    def test_progress_watch_detects_stall_and_resets_only_on_progress(self):
        soak = load_soak()
        watch = soak.ProgressWatch(5)
        self.assertFalse(watch.observe(0, {"lines": 10}))
        self.assertFalse(watch.observe(4, {"lines": 10}))
        self.assertTrue(watch.observe(5, {"lines": 10}))
        self.assertFalse(watch.observe(6, {"lines": 20}))
        self.assertFalse(watch.observe(10, {"lines": 20}))
        self.assertEqual(len(watch.stalls), 1)
        self.assertEqual(watch.stalls[0]["duration_seconds"], 6)
        self.assertFalse(watch.observe(11, {"lines": 30}))
        self.assertEqual(watch.max_gap_seconds, 6)

    def test_memory_trend_excludes_warmup_and_reports_median_delta(self):
        soak = load_soak()
        samples = [{"elapsed_seconds": t, "app": {"private_bytes": value}}
                   for t, value in [(0, 10000), (1, 100), (2, 110), (3, 120), (4, 130)]]
        trend = soak.memory_trend(samples, "app", "private_bytes", 1)
        self.assertEqual(trend["samples"], 4)
        self.assertEqual(trend["early_median_bytes"], 100)
        self.assertEqual(trend["late_median_bytes"], 130)
        self.assertEqual(trend["median_delta_bytes"], 30)
        self.assertAlmostEqual(trend["slope_bytes_per_second"], 10)

    @unittest.skipUnless(os.name == "nt", "Windows process resource APIs")
    def test_windows_reader_samples_live_process_and_handles_exit(self):
        soak = load_soak()
        with subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"]) as process:
            with soak.ProcessReader(process.pid) as reader:
                sample = reader.sample()
                self.assertEqual(sample["pid"], process.pid)
                self.assertTrue(sample["alive"])
                for field in ["private_bytes", "rss_bytes", "handles", "threads"]:
                    self.assertGreater(sample[field], 0, field)
                self.assertGreaterEqual(sample["cpu_seconds"], 0)
                process.terminate()
                process.wait(timeout=3)
                self.assertFalse(reader.sample()["alive"])


if __name__ == "__main__":
    unittest.main()
