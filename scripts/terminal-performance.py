"""Compare real macOS terminal GUIs with one isolated, shared PTY workload.

Uses Python stdlib and macOS libproc. Records raw samples and config files.
DSR roundtrips acknowledge terminal parsing, NOT GPU presentation or typing.
No user's shell profile/configuration is loaded, and only owned apps are closed.
"""

import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import select
import signal
import statistics
import subprocess
import sys
import time
import plistlib


ESC = b"\x1b"


def write_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def percentile(values, percent):
    return sorted(values)[math.ceil(len(values) * percent / 100) - 1]


def summary(values):
    return {"min": min(values), "p50": percentile(values, 50),
            "p95": percentile(values, 95), "max": max(values)}


def payload(start, count, workload):
    content = "한글 가나다라마바사 / 界 / e\u0301 / 🧪 / -> != ===" if workload == "unicode" else "ASCII output abcdefghijklmnopqrstuvwxyz -> != ==="
    return "".join(f"\x1b[38;2;151;219;174msoak {index:012d}\x1b[0m {content}\r\n"
                   for index in range(start, start + count)).encode("utf-8")


def write_all(data):
    view = memoryview(data)
    while view:
        count = os.write(sys.stdout.fileno(), view)
        if count <= 0:
            raise OSError("PTY write made no progress")
        view = view[count:]


def dsr(timeout=10):
    """Flush preceding output through the parser; own raw stdin has no user input."""
    write_all(ESC + b"[6n")
    deadline = time.monotonic() + timeout
    received = b""
    while time.monotonic() < deadline:
        if select.select([sys.stdin], [], [], max(0, deadline - time.monotonic()))[0]:
            data = os.read(sys.stdin.fileno(), 1024)
            if not data:
                raise OSError("PTY stdin closed while waiting for DSR")
            received += data
            match = re.search(rb"\x1b\[(\d+);(\d+)R", received)
            if match:
                return [int(match[1]), int(match[2])]
            if len(received) > 4096:
                raise OSError("Unexpected input instead of DSR")
    raise TimeoutError("Terminal did not acknowledge DSR")


def fixture(args):
    import termios
    import tty
    directory = args.output.resolve()
    original = termios.tcgetattr(sys.stdin)
    tty.setraw(sys.stdin.fileno())
    state = {"pid": os.getpid(), "ppid": os.getppid(), "phase": "starting"}

    def status(phase, **values):
        state.update(phase=phase, grid=list(os.get_terminal_size()), **values)
        write_json(directory / "fixture.json", state)

    def wait_for(name):
        deadline = time.monotonic() + 600
        while not (directory / name).exists():
            if (directory / "stop").exists() or time.monotonic() > deadline:
                raise TimeoutError(f"Fixture stopped while waiting for {name}")
            time.sleep(0.05)

    try:
        # Startup grid can change after the first native viewport layout.
        time.sleep(2)
        dsr()
        status("idle")
        wait_for("soak")
        status("soak")
        started = time.monotonic()
        lines = 0
        for _ in range(round(args.seconds * 20)):
            tick = time.monotonic()
            write_all(payload(lines, 30, args.workload))
            lines += 30
            time.sleep(max(0, 0.05 - (time.monotonic() - tick)))
        dsr()
        status("soak_done", soak_lines=lines, soak_seconds=time.monotonic() - started)
        wait_for("burst")
        data = payload(0, args.burst_lines, args.workload)
        status("burst")
        started = time.monotonic()
        write_all(data)
        dsr(timeout=60)
        elapsed = time.monotonic() - started
        status("burst_done", burst_bytes=len(data), burst_seconds=elapsed,
               burst_mib_per_second=len(data) / elapsed / (1024 ** 2),
               payload_sha256=hashlib.sha256(data).hexdigest())
        wait_for("roundtrip")
        status("roundtrip")
        latencies = []
        for index in range(110):
            started = time.monotonic_ns()
            write_all(b"x\r")
            dsr()
            elapsed = (time.monotonic_ns() - started) / 1e6
            if index >= 10:
                latencies.append(elapsed)
            time.sleep(0.01)
        status("done", parser_roundtrip_ms=summary(latencies),
               parser_roundtrip_samples=latencies)
        wait_for("stop")
    except BaseException as error:
        status("failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        termios.tcsetattr(sys.stdin, termios.TCSANOW, original)


class Usage(ctypes.Structure):
    # Exact rusage_info_v2 layout from the macOS SDK's sys/resource.h.
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in [
        "user_ns", "system_ns", "idle_wakeups", "interrupt_wakeups", "pageins",
        "wired_bytes", "rss_bytes", "footprint_bytes", "start_abstime", "exit_abstime",
        "child_user_ns", "child_system_ns", "child_idle_wakeups", "child_interrupt_wakeups",
        "child_pageins", "child_elapsed_abstime", "disk_read_bytes", "disk_write_bytes"]]


class Reader:
    def __init__(self):
        self.lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        self.lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.POINTER(Usage)]
        self.lib.proc_pid_rusage.restype = ctypes.c_int

    def sample(self, pid):
        usage = Usage()
        if self.lib.proc_pid_rusage(pid, 2, ctypes.byref(usage)):
            raise OSError(ctypes.get_errno(), f"proc_pid_rusage({pid})")
        if usage.exit_abstime:
            raise OSError(f"Process {pid} exited")
        return {"pid": pid, "start_abstime": usage.start_abstime,
                "rss_bytes": usage.rss_bytes, "footprint_bytes": usage.footprint_bytes,
                "cpu_seconds": (usage.user_ns + usage.system_ns) / 1e9}


def ancestry(pid):
    for _ in range(8):
        result = subprocess.run(["ps", "-p", str(pid), "-o", "ppid=,comm="],
                                check=True, capture_output=True, text=True).stdout.strip()
        parent, command = result.split(maxsplit=1)
        yield pid, command
        pid = int(parent)
        if pid <= 1:
            return


def state_at(directory):
    try:
        return json.loads((directory / "fixture.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def capture(directory, name, pid):
    for command in [["vmmap", "-summary", str(pid)], ["heap", "-s", "--noContent", str(pid)]]:
        with (directory / f"{name}-{command[0]}.txt").open("w") as stream:
            result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, timeout=40)
        if result.returncode:
            raise RuntimeError(f"Memory capture failed: {command}")


def window_info(pid):
    helper = Path(__file__).with_name("terminal-window-info.swift")
    command = ["swift", str(helper), str(pid)]
    return json.loads(subprocess.check_output(command, text=True, timeout=30))


def run_one(args, product, directory, grid):
    directory.mkdir(parents=True, exist_ok=False)
    child = [sys.executable, "-u", str(Path(__file__).resolve()), "--child", "--output", str(directory),
             "--seconds", str(args.seconds), "--burst-lines", str(args.burst_lines),
             "--workload", args.workload]
    if product == "whostty":
        config = directory / "config.toml"
        config.write_text(f"font_size = 15.0\nscrollback = {args.scrollback}\n", encoding="utf-8")
        command = ["open", "-na", str(args.bundle), "--args", "--config", str(config), "-e", *child]
    else:
        config = directory / "config.ghostty"
        config.write_text("font-family = Menlo\nfont-size = 15\nshell-integration = none\n"
                          "window-save-state = never\nbackground-opacity = 1\nwindow-vsync = true\n"
                          "confirm-close-surface = false\nquit-after-last-window-closed = true\n"
                          f"scrollback-limit = {args.ghostty_scrollback_bytes}\n"
                          f"window-width = {grid[0]}\nwindow-height = {grid[1]}\n", encoding="utf-8")
        command = ["open", "-na", str(args.ghostty_app), "--args", "--config-default-files=false",
                   f"--config-file={config}", "-e", *child]
    reader = Reader()
    samples = []
    app_pid = None
    app_identity = None
    app = None
    started = time.monotonic()
    with (directory / "app.log").open("w") as log:
        app = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            while time.monotonic() - started < 30:
                state = state_at(directory)
                if state and state["phase"] == "idle":
                    for pid, binary in ancestry(state["pid"]):
                        if Path(binary).name.lower() == product:
                            app_pid = pid
                            break
                    if app_pid is None:
                        raise RuntimeError("Could not find owned terminal ancestor")
                    app_identity = reader.sample(app_pid)["start_abstime"]
                    break
                time.sleep(0.1)
            if app_pid is None:
                raise TimeoutError("GUI fixture did not start")
            if grid is not None and state["grid"] != grid:
                raise RuntimeError(f"Grid mismatch: {state['grid']} != {grid}")
            grid = state["grid"]
            print(json.dumps({"event": "started", "product": product, "pid": app_pid,
                              "grid": grid, "output": str(directory)}), flush=True)
            # Foreground and background terminals can schedule rendering
            # differently. Never silently compare different window states.
            end = time.monotonic() + 120
            while time.monotonic() < end:
                window = window_info(app_pid)
                active = window["frontmost_pid"] == app_pid
                if active == (args.window_state == "foreground") and window["windows"]:
                    break
                time.sleep(1)
            else:
                raise TimeoutError(f"Set the benchmark window to {args.window_state} within 120 seconds")

            def sample():
                current = state_at(directory)
                if current and current["phase"] == "failed":
                    raise RuntimeError(current["error"])
                value = {"time": time.monotonic(), "phase": current["phase"] if current else None,
                         "app": reader.sample(app_pid), "fixture": reader.sample(state["pid"])}
                if value["app"]["start_abstime"] != app_identity:
                    raise RuntimeError("Terminal process identity changed")
                samples.append(value)
                with (directory / "samples.jsonl").open("a", encoding="utf-8") as stream:
                    stream.write(json.dumps(value) + "\n")
                return current

            def settle(seconds):
                end = time.monotonic() + seconds
                while time.monotonic() < end:
                    sample()
                    time.sleep(0.25)

            def until(phase, timeout):
                end = time.monotonic() + timeout
                while time.monotonic() < end:
                    current = sample()
                    if current and current["phase"] == phase:
                        return current
                    time.sleep(0.25)
                raise TimeoutError(f"Waiting for {phase}")

            settle(5)
            window = window_info(app_pid)
            write_json(directory / "window.json", window)
            if (window["frontmost_pid"] == app_pid) != (args.window_state == "foreground") or not window["windows"]:
                raise RuntimeError(f"Owned terminal window state changed: {window}")
            idle = reader.sample(app_pid)
            if args.capture:
                capture(directory, "idle", app_pid)
            before = reader.sample(app_pid)
            soak_started = time.monotonic()
            (directory / "soak").touch()
            state = until("soak_done", args.seconds + 30)
            soak_elapsed = time.monotonic() - soak_started
            after = reader.sample(app_pid)
            soak_cpu_percent = 100 * (after["cpu_seconds"] - before["cpu_seconds"]) / soak_elapsed
            settle(3)
            soak_memory = reader.sample(app_pid)
            if args.capture:
                capture(directory, "soak", app_pid)
            (directory / "burst").touch()
            state = until("burst_done", 90)
            settle(3)
            burst_memory = reader.sample(app_pid)
            if args.capture:
                capture(directory, "burst", app_pid)
            (directory / "roundtrip").touch()
            state = until("done", 60)
            end_window = window_info(app_pid)
            if (end_window["frontmost_pid"] == app_pid) != (args.window_state == "foreground"):
                raise RuntimeError("Window state changed during the measurement")
            report = {"product": product, "command": command, "grid": grid, "window": window,
                      "end_window": end_window, "window_state": args.window_state,
                      "workload": args.workload, "scrollback_lines": args.scrollback if product == "whostty" else None,
                      "scrollback_bytes": args.ghostty_scrollback_bytes if product == "ghostty" else None,
                      "idle": idle, "after_soak": soak_memory, "after_burst": burst_memory,
                      "soak_app_cpu_percent": soak_cpu_percent, "fixture": state,
                      "samples": samples}
            write_json(directory / "report.json", report)
            print(json.dumps({"event": "completed", "product": product,
                              "soak_cpu_percent": soak_cpu_percent,
                              "footprint_mib": soak_memory["footprint_bytes"] / 1024 ** 2,
                              "burst_mib_per_second": state["burst_mib_per_second"]}), flush=True)
            return report
        finally:
            (directory / "stop").touch()
            # Only signal the exact ancestor identified from our own fixture.
            if app_pid is not None:
                try:
                    if reader.sample(app_pid)["start_abstime"] == app_identity:
                        os.kill(app_pid, signal.SIGTERM)
                except OSError:
                    pass
            if app.poll() is None:
                try:
                    app.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    app.terminate()
                    app.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--whostty", type=Path, default=Path("target/release/whostty"))
    parser.add_argument("--ghostty-app", type=Path, default=Path("/Applications/Ghostty.app"))
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--burst-lines", type=int, default=200000)
    parser.add_argument("--scrollback", type=int, default=0)
    parser.add_argument("--ghostty-scrollback-bytes", type=int, default=0)
    parser.add_argument("--workload", choices=["ascii", "unicode"], default="unicode")
    parser.add_argument("--capture", action="store_true")
    parser.add_argument("--window-state", choices=["foreground", "background"], default="background")
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not math.isfinite(args.seconds) or not 1 <= args.seconds <= 3600:
        parser.error("--seconds must be finite and between 1 and 3600")
    if not 1 <= args.repeat <= 20 or not 1 <= args.burst_lines <= 2000000:
        parser.error("repeat or burst line count out of range")
    if not 0 <= args.scrollback <= 100000 or not 0 <= args.ghostty_scrollback_bytes <= 1000000000:
        parser.error("scrollback limit out of range")
    if args.child:
        fixture(args)
        return
    if sys.platform != "darwin":
        parser.error("the comparison monitor requires macOS")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    # LaunchServices gives the native test binary a normal, independently
    # identifiable app/window without installing or changing the user's apps.
    args.bundle = args.output / "Whostty Benchmark.app"
    executable = args.bundle / "Contents/MacOS/whostty"
    executable.parent.mkdir(parents=True)
    executable.symlink_to(args.whostty.resolve())
    with (args.bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundleIdentifier": "com.whostty.performance",
                      "CFBundleName": "Whostty Benchmark", "CFBundleExecutable": "whostty",
                      "CFBundlePackageType": "APPL", "NSHighResolutionCapable": True}, stream)
    binary = args.ghostty_app / "Contents/MacOS/ghostty"
    metadata = {"platform": platform.platform(), "python": sys.version,
                "whostty_sha256": hashlib.sha256(args.whostty.read_bytes()).hexdigest(),
                "ghostty_version": subprocess.check_output([str(binary), "+version"], text=True),
                "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "window_state": args.window_state,
                "fixture_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "limitations": ["Same Menlo 15pt, grid, Python fixture and payload; native font metrics can differ.",
                    "Zero scrollback isolates rendering/PTY costs; nonzero limits use different product units.",
                    "App CPU excludes fixture and window server; one logical CPU core is 100%.",
                    "macOS physical footprint and RSS are not Windows PrivateUsage.",
                    "DSR acknowledges the parser, not GPU display or physical keyboard latency.",
                    "Desktop load and OS scheduling are not isolated; profile captures run outside timed phases."]}
    write_json(args.output / "metadata.json", metadata)
    reports = []
    grid = None
    for index in range(args.repeat):
        # First Whostty determines its fixed default grid; alternate later pairs.
        products = ["whostty", "ghostty"] if index % 2 == 0 else ["ghostty", "whostty"]
        for product in products:
            report = run_one(args, product, args.output / f"{index + 1}-{product}", grid)
            grid = report["grid"]
            reports.append(report)
    result = {"metadata": metadata, "grid": grid, "runs": reports, "medians": {}}
    for product in ["whostty", "ghostty"]:
        group = [report for report in reports if report["product"] == product]
        result["medians"][product] = {
            "idle_footprint_mib": statistics.median(r["idle"]["footprint_bytes"] / 1024 ** 2 for r in group),
            "soak_footprint_mib": statistics.median(r["after_soak"]["footprint_bytes"] / 1024 ** 2 for r in group),
            "soak_rss_mib": statistics.median(r["after_soak"]["rss_bytes"] / 1024 ** 2 for r in group),
            "soak_cpu_percent": statistics.median(r["soak_app_cpu_percent"] for r in group),
            "burst_mib_per_second": statistics.median(r["fixture"]["burst_mib_per_second"] for r in group),
            "parser_roundtrip_p50_ms": statistics.median(r["fixture"]["parser_roundtrip_ms"]["p50"] for r in group),
            "parser_roundtrip_p95_ms": statistics.median(r["fixture"]["parser_roundtrip_ms"]["p95"] for r in group)}
    hashes = {r["fixture"]["payload_sha256"] for r in reports}
    if len(hashes) != 1:
        raise RuntimeError("Compared terminals did not receive identical burst payloads")
    write_json(args.output / "comparison.json", result)
    print(json.dumps(result["medians"], indent=2), flush=True)


if __name__ == "__main__":
    main()
