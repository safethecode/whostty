import ctypes
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).with_name("terminal-performance.py")
spec = importlib.util.spec_from_file_location("terminal_performance", SCRIPT)
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class TerminalPerformanceTest(unittest.TestCase):
    def test_unix_fixture_requires_real_dsr_acknowledgements(self):
        if os.name == "nt":
            self.skipTest("POSIX PTY fixture")
        import fcntl
        import json
        import pty
        import select
        import struct
        import termios
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        try:
            with tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                command = [sys.executable, str(SCRIPT), "--child", "--output", temporary,
                           "--seconds", "1", "--burst-lines", "100"]
                with subprocess.Popen(command, stdin=slave, stdout=slave, stderr=subprocess.PIPE) as process:
                    received = b""
                    replies = 0
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        if select.select([master], [], [], 0.05)[0]:
                            received += os.read(master, 65536)
                            while b"\x1b[6n" in received:
                                before, received = received.split(b"\x1b[6n", 1)
                                self.assertNotIn(b"Traceback", before)
                                os.write(master, b"\x1b[24;1R")
                                replies += 1
                        state = bench.state_at(directory)
                        if state:
                            if state["phase"] == "idle":
                                (directory / "soak").touch()
                            elif state["phase"] == "soak_done":
                                (directory / "burst").touch()
                            elif state["phase"] == "burst_done":
                                (directory / "roundtrip").touch()
                            elif state["phase"] == "done":
                                break
                            elif state["phase"] == "failed":
                                self.fail(state["error"])
                    else:
                        process.terminate()
                        self.fail("Fixture did not complete")
                    (directory / "stop").touch()
                    _, error = process.communicate(timeout=3)
                    self.assertEqual(process.returncode, 0, error.decode(errors="replace"))
                    self.assertEqual(state["grid"], [80, 24])
                    self.assertEqual(state["soak_lines"], 600)
                    self.assertEqual(state["burst_bytes"], len(bench.payload(0, 100, "unicode")))
                    self.assertEqual(len(state["parser_roundtrip_samples"]), 100)
                    self.assertEqual(replies, 113)
        finally:
            os.close(master)
            os.close(slave)

    @unittest.skipUnless(sys.platform == "darwin", "macOS resource API")
    def test_libproc_fields_match_a_live_child_and_report_cpu_and_memory(self):
        reader = bench.Reader()
        self.assertEqual(ctypes.sizeof(bench.Usage), 160)
        before = reader.sample(os.getpid())
        # Touch real pages and retain them through the second sample.
        allocation = bytearray(8 * 1024 * 1024)
        for offset in range(0, len(allocation), 4096):
            allocation[offset] = 1
        after = reader.sample(os.getpid())
        self.assertEqual(before["start_abstime"], after["start_abstime"])
        self.assertGreater(after["rss_bytes"], 0)
        self.assertGreater(after["footprint_bytes"], 0)
        self.assertGreaterEqual(after["cpu_seconds"], before["cpu_seconds"])


if __name__ == "__main__":
    unittest.main()
