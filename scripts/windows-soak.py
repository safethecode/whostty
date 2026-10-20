"""Sustained output through a real Whostty GUI, using only Python's stdlib.

Example (PowerShell):
  python scripts/windows-soak.py --exe C:/path/to/whostty.exe --duration 1800

The dedicated GUI remains open after success. Close it after inspecting the
result. A stop file in the output directory requests graceful fixture shutdown.
Progress and resource samples are proxies: they do not measure GPU frames,
rendered output correctness, IME behavior, or physical input-to-display latency.
"""

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time
import uuid


ESC = "\x1b"


def bounded_number(minimum, maximum, integer=False):
    def parse(value):
        try:
            number = int(value) if integer else float(value)
        except ValueError as error:
            raise argparse.ArgumentTypeError("must be a number") from error
        if not math.isfinite(number) or not minimum <= number <= maximum:
            raise argparse.ArgumentTypeError(f"must be finite and between {minimum} and {maximum}")
        return number
    return parse


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--exe", type=Path, help="normal Windows GUI executable")
    parser.add_argument("--output", type=Path, help="new output directory (default: unique temp directory)")
    parser.add_argument("--duration", type=bounded_number(0.1, 86400), default=1800.0)
    parser.add_argument("--lines-per-second", type=bounded_number(10, 2000, True), default=600)
    parser.add_argument("--sample-seconds", type=bounded_number(0.1, 60), default=1.0)
    parser.add_argument("--warmup-seconds", type=bounded_number(0, 3600), default=30.0)
    parser.add_argument("--stall-seconds", type=bounded_number(1, 300), default=5.0)
    parser.add_argument("--startup-seconds", type=bounded_number(1, 120), default=30.0)
    parser.add_argument("--shutdown-seconds", type=bounded_number(1, 120), default=10.0)
    parser.add_argument("--alternate-interval", type=bounded_number(0, 3600), default=10.0,
                        help="seconds between normal/alternate screen switches; 0 disables")
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--token", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.child:
        if args.output is None or not args.token:
            parser.error("--child requires --output and --token")
    elif args.exe is None:
        parser.error("--exe is required")
    elif args.stall_seconds < 2 * args.sample_seconds:
        parser.error("--stall-seconds must be at least twice --sample-seconds")
    return args


def atomic_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    # Windows readers may briefly hold the destination without FILE_SHARE_DELETE.
    for attempt in range(6):
        try:
            temporary.replace(path)
            return
        except PermissionError:
            if attempt == 5:
                raise
            time.sleep(0.005 * (attempt + 1))


def read_status(directory, token):
    try:
        status = json.loads((directory / "child-status.json").read_text(encoding="utf-8"))
        if status.get("token") == token and isinstance(status.get("pid"), int) and status["pid"] > 0:
            return status
    except (OSError, ValueError, TypeError):
        pass
    return None


def run_child(args):
    directory = args.output.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    status = {"token": args.token, "pid": os.getpid(), "state": "running", "lines": 0,
              "bytes": 0, "screen_switches": 0, "reason": None, "exit_code": None}
    alternate = False
    stream = sys.stdout.buffer

    def heartbeat():
        status.update(elapsed_seconds=time.monotonic() - start, updated_unix_seconds=time.time())
        atomic_json(directory / "child-status.json", status)

    def emit(payload, lines=0):
        stream.write(payload)
        stream.flush()
        status["bytes"] += len(payload)
        status["lines"] += lines

    heartbeat()
    batch_lines = max(1, int(args.lines_per_second * 0.05))
    period = batch_lines / args.lines_per_second
    deadline = start
    next_status = start
    next_switch = start + args.alternate_interval
    try:
        while True:
            now = time.monotonic()
            if (directory / "stop").exists():
                status["reason"] = "stop_requested"
                break
            if now - start >= args.duration:
                status["reason"] = "duration"
                break
            if args.alternate_interval and now >= next_switch:
                alternate = not alternate
                emit((f"{ESC}[?1049h{ESC}[2J{ESC}[H" if alternate else f"{ESC}[?1049l").encode())
                status["screen_switches"] += 1
                next_switch = now + args.alternate_interval
            first = status["lines"]
            payload = "".join(
                f"{ESC}[38;2;151;219;174msoak {first + index:012d}{ESC}[0m "
                f"한글 가나다라마바사 / 界 / e\u0301 / 🧪 / -> != === | ANSI Unicode output\r\n"
                for index in range(batch_lines)
            ).encode("utf-8")
            emit(payload, batch_lines)
            now = time.monotonic()
            if now >= next_status:
                heartbeat()
                next_status = now + 0.25
            # Do not flood the PTY with catch-up batches after an output block.
            deadline = max(deadline + period, now)
            time.sleep(min(max(0, deadline - now), max(0, start + args.duration - now)))
        emit((f"{ESC}[?1049l{ESC}[0m" if alternate else f"{ESC}[0m").encode())
        status.update(state="completed", exit_code=0)
        heartbeat()
        return 0
    except (OSError, KeyboardInterrupt) as error:
        status.update(state="failed", reason=type(error).__name__, error=str(error), exit_code=1)
        heartbeat()
        return 1


class ProcessReader:
    """Retain a read-only process handle, avoiding PID reuse between samples."""

    def __init__(self, pid):
        if os.name != "nt":
            raise OSError("Windows process APIs are required")
        self.pid = pid
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.restype = wintypes.BOOL
        self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self.kernel.WaitForSingleObject.restype = wintypes.DWORD
        self.kernel.GetProcessHandleCount.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.GetProcessHandleCount.restype = wintypes.BOOL
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        self.kernel.GetProcessTimes.restype = wintypes.BOOL
        self.kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.GetExitCodeProcess.restype = wintypes.BOOL
        self.kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        self.kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        self.handle = self.kernel.OpenProcess(0x00100000 | 0x0400 | 0x0010, False, pid)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        self.previous = None

    def __enter__(self):
        return self

    def __exit__(self, *unused):
        self.close()

    def close(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None

    def thread_count(self):
        class ThreadEntry(ctypes.Structure):
            _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
                        ("th32ThreadID", wintypes.DWORD), ("th32OwnerProcessID", wintypes.DWORD),
                        ("tpBasePri", wintypes.LONG), ("tpDeltaPri", wintypes.LONG),
                        ("dwFlags", wintypes.DWORD)]
        self.kernel.Thread32First.argtypes = [wintypes.HANDLE, ctypes.POINTER(ThreadEntry)]
        self.kernel.Thread32First.restype = wintypes.BOOL
        self.kernel.Thread32Next.argtypes = self.kernel.Thread32First.argtypes
        self.kernel.Thread32Next.restype = wintypes.BOOL
        snapshot = self.kernel.CreateToolhelp32Snapshot(0x00000004, 0)
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        count = 0
        try:
            entry = ThreadEntry()
            entry.dwSize = ctypes.sizeof(entry)
            more = self.kernel.Thread32First(snapshot, ctypes.byref(entry))
            while more:
                count += entry.th32OwnerProcessID == self.pid
                more = self.kernel.Thread32Next(snapshot, ctypes.byref(entry))
            if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            self.kernel.CloseHandle(snapshot)
        return count

    def sample(self):
        alive = self.kernel.WaitForSingleObject(self.handle, 0) == 258  # WAIT_TIMEOUT
        if not alive:
            code = wintypes.DWORD()
            if not self.kernel.GetExitCodeProcess(self.handle, ctypes.byref(code)):
                raise ctypes.WinError(ctypes.get_last_error())
            return {"pid": self.pid, "alive": False, "exit_code": code.value}

        class MemoryCounters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ["PeakWorkingSetSize", "WorkingSetSize",
                "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage", "PrivateUsage"]]
        memory = MemoryCounters()
        memory.cb = ctypes.sizeof(memory)
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
        self.psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        if not self.psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(memory), memory.cb):
            if self.kernel.WaitForSingleObject(self.handle, 0) == 0:
                return self.sample()
            raise ctypes.WinError(ctypes.get_last_error())
        handles = wintypes.DWORD()
        times = [wintypes.FILETIME() for _ in range(4)]
        if not self.kernel.GetProcessHandleCount(self.handle, ctypes.byref(handles)):
            raise ctypes.WinError(ctypes.get_last_error())
        if not self.kernel.GetProcessTimes(self.handle, *[ctypes.byref(value) for value in times]):
            raise ctypes.WinError(ctypes.get_last_error())
        ticks = lambda value: (value.dwHighDateTime << 32) | value.dwLowDateTime
        cpu = (ticks(times[2]) + ticks(times[3])) / 10_000_000
        now = time.monotonic()
        percent = None
        if self.previous and now > self.previous[0]:
            percent = max(0, (cpu - self.previous[1]) / (now - self.previous[0]) * 100)
        self.previous = (now, cpu)
        return {"pid": self.pid, "alive": True, "private_bytes": memory.PrivateUsage,
                "rss_bytes": memory.WorkingSetSize, "handles": handles.value,
                "threads": self.thread_count(), "cpu_seconds": cpu,
                "cpu_percent_one_core": percent, "creation_filetime": ticks(times[0])}


class ProgressWatch:
    def __init__(self, threshold):
        self.threshold = threshold
        self.lines = None
        self.last_progress = None
        self.max_gap_seconds = 0.0
        self.stalls = []
        self.active_stall = None

    def observe(self, now, status):
        if self.last_progress is None:
            self.last_progress = now
            self.lines = status["lines"]
            return False
        gap = now - self.last_progress
        self.max_gap_seconds = max(self.max_gap_seconds, gap)
        if status["lines"] > self.lines:
            if self.active_stall is not None:
                self.active_stall["duration_seconds"] = gap
                self.active_stall = None
            self.lines = status["lines"]
            self.last_progress = now
            return False
        if gap >= self.threshold:
            if self.active_stall is None:
                self.active_stall = {"since_elapsed_seconds": self.last_progress, "duration_seconds": gap}
                self.stalls.append(self.active_stall)
            self.active_stall["duration_seconds"] = gap
            return True
        return False


def memory_trend(samples, process, field, warmup):
    points = [(sample["elapsed_seconds"], sample[process][field]) for sample in samples
              if sample["elapsed_seconds"] >= warmup and field in (sample.get(process) or {})]
    if len(points) < 2 or points[-1][0] <= points[0][0]:
        return {"samples": len(points), "available": False}
    window = min(60.0, (points[-1][0] - points[0][0]) / 3)
    early = statistics.median(value for t, value in points if t < points[0][0] + window)
    late = statistics.median(value for t, value in points if t > points[-1][0] - window)
    mean_t = statistics.mean(t for t, _ in points)
    mean_value = statistics.mean(value for _, value in points)
    denominator = sum((t - mean_t) ** 2 for t, _ in points)
    slope = sum((t - mean_t) * (value - mean_value) for t, value in points) / denominator if denominator else None
    return {"available": True, "samples": len(points), "warmup_seconds": warmup,
            "window_seconds": window, "early_median_bytes": early, "late_median_bytes": late,
            "median_delta_bytes": late - early, "slope_bytes_per_second": slope,
            "minimum_bytes": min(value for _, value in points),
            "maximum_bytes": max(value for _, value in points)}


def resource_summary(samples, name, warmup):
    data = [sample[name] for sample in samples if (sample.get(name) or {}).get("alive")]
    result = {"sample_count": len(data), "memory": {
        field: memory_trend(samples, name, field, warmup) for field in ["private_bytes", "rss_bytes"]}}
    for field in ["handles", "threads", "cpu_percent_one_core"]:
        values = [item[field] for item in data if item.get(field) is not None]
        result[field] = {"first": values[0], "last": values[-1], "maximum": max(values),
                         "median": statistics.median(values)} if values else None
    if len(data) >= 2:
        result["cpu_seconds_delta"] = data[-1]["cpu_seconds"] - data[0]["cpu_seconds"]
    return result


def stop_owned_tree(app):
    """No image-name matching: only the still-live Popen-owned GUI and descendants."""
    if app.poll() is not None:
        return "app_already_exited"
    try:
        result = subprocess.run(["taskkill.exe", "/PID", str(app.pid), "/T", "/F"],
                                capture_output=True, timeout=5, creationflags=subprocess.CREATE_NO_WINDOW)
        app.wait(timeout=5)
        return f"owned_tree_taskkill_exit_{result.returncode}"
    except (OSError, subprocess.TimeoutExpired) as error:
        return f"cleanup_error: {error}"


def run_monitor(args):
    if os.name != "nt":
        raise OSError("the GUI soak monitor requires Windows")
    executable = args.exe.resolve(strict=True)
    if not executable.is_file():
        raise OSError("--exe must be a file")
    if args.output is None:
        directory = Path(tempfile.mkdtemp(prefix="whostty-soak-"))
    else:
        directory = args.output.resolve()
        directory.mkdir(parents=True, exist_ok=False)
    config = directory / "config.toml"
    config.write_text("# Isolated soak configuration; never the user's config.\nfont_size = 15.0\nscrollback = 10000\n", encoding="utf-8")
    token = uuid.uuid4().hex
    command = [str(executable), "--config", str(config), "-d", str(directory), "-e",
               sys.executable, "-u", str(Path(__file__).resolve()), "--child", "--output", str(directory),
               "--token", token, "--duration", str(args.duration),
               "--lines-per-second", str(args.lines_per_second),
               "--alternate-interval", str(args.alternate_interval)]
    start = time.monotonic()
    samples = []
    errors = []
    status = None
    child_reader = None
    app_reader = None
    app = None
    watch = ProgressWatch(args.stall_seconds)
    reason = "startup_timeout"
    cleanup = None
    fixture_exit_code = None
    metadata = {"schema_version": 1, "started_unix_seconds": time.time(), "command": command,
                "binary_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                "fixture_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "python_version": sys.version, "platform": platform.platform(),
                "duration_seconds": args.duration, "lines_per_second": args.lines_per_second,
                "sample_seconds": args.sample_seconds, "warmup_seconds": args.warmup_seconds,
                "stall_seconds": args.stall_seconds, "logical_cpu_count": os.cpu_count()}
    atomic_json(directory / "run.json", metadata)
    try:
        with (directory / "app.log").open("wb") as log, (directory / "samples.jsonl").open("w", encoding="utf-8") as journal:
            app = subprocess.Popen(command, stdout=log, stderr=log)
            app_reader = ProcessReader(app.pid)
            print(json.dumps({"event": "started", "app_pid": app.pid, "output": str(directory)}), flush=True)
            next_sample = time.monotonic()
            while True:
                now = time.monotonic()
                elapsed = now - start
                incoming = read_status(directory, token)
                if incoming is not None:
                    status = incoming
                if child_reader is None and status is not None:
                    try:
                        child_reader = ProcessReader(status["pid"])
                    except OSError as error:
                        if status["state"] != "completed":
                            raise error
                sample = {"elapsed_seconds": elapsed, "unix_seconds": time.time(),
                          "app": app_reader.sample(),
                          "child": child_reader.sample() if child_reader else None,
                          "progress": status,
                          "heartbeat_age_seconds": time.time() - status["updated_unix_seconds"] if status else None}
                samples.append(sample)
                journal.write(json.dumps(sample, ensure_ascii=False) + "\n")
                journal.flush()
                if status is not None:
                    stalled = watch.observe(elapsed, status)
                    if status["state"] in ("completed", "failed"):
                        reason = "fixture_completed" if status["state"] == "completed" else "fixture_failed"
                        break
                    if stalled:
                        reason = "progress_stall"
                        break
                    if sample["child"] and not sample["child"]["alive"]:
                        reason = "fixture_exited_without_completion"
                        break
                if app.poll() is not None:
                    reason = "app_exited_early"
                    break
                if status is None and elapsed >= args.startup_seconds:
                    reason = "startup_timeout"
                    break
                if elapsed >= args.duration + args.startup_seconds + args.shutdown_seconds:
                    reason = "duration_timeout"
                    break
                if (directory / "stop").exists():
                    reason = "stop_requested"
                    break
                next_sample = max(next_sample + args.sample_seconds, time.monotonic())
                time.sleep(max(0, next_sample - time.monotonic()))
    except KeyboardInterrupt:
        reason = "interrupted"
    except (OSError, ValueError) as error:
        reason = "monitor_error"
        errors.append(f"{type(error).__name__}: {error}")
    finally:
        # Even a blocked fixture receives a graceful request before bounded cleanup.
        (directory / "stop").touch()
        deadline = time.monotonic() + args.shutdown_seconds
        while app is not None and time.monotonic() < deadline:
            final_status = read_status(directory, token)
            if final_status is not None:
                status = final_status
            if status and status["state"] in ("completed", "failed"):
                if child_reader is None:
                    break
                final_child = child_reader.sample()
                if not final_child["alive"]:
                    fixture_exit_code = final_child["exit_code"]
                    break
            if app.poll() is not None:
                break
            time.sleep(0.1)
        if child_reader:
            child_reader.close()
        if app_reader:
            app_reader.close()
        if reason == "fixture_completed" and fixture_exit_code != 0:
            reason = "fixture_exit_unconfirmed"
        if reason == "fixture_completed" and app.poll() is not None:
            reason = "app_exited_at_completion"
        # A successful run leaves the dedicated GUI available for visual inspection.
        if app is not None and reason != "fixture_completed":
            cleanup = stop_owned_tree(app)

    completed = bool(reason == "fixture_completed" and fixture_exit_code == 0 and status and status["exit_code"] == 0
                     and status["reason"] == "duration"
                     and status["elapsed_seconds"] >= args.duration and not watch.stalls)
    report = {**metadata, "output": str(directory), "completed": completed, "reason": reason,
              "elapsed_seconds": time.monotonic() - start, "app_pid": app.pid if app else None,
              "child_pid": status["pid"] if status else None,
              "app_alive_at_report": app is not None and app.poll() is None,
              "app_exit_code": app.poll() if app else None, "fixture_exit_code": fixture_exit_code, "fixture": status,
              "observed_lines_per_second": status["lines"] / status["elapsed_seconds"] if status else None,
              "observed_source_bytes_per_second": status["bytes"] / status["elapsed_seconds"] if status else None,
              "sample_count": len(samples), "cleanup": cleanup, "errors": errors,
              "progress_proxy": {"stall_threshold_seconds": args.stall_seconds,
                  "maximum_observed_gap_seconds": watch.max_gap_seconds, "stalls": watch.stalls},
              "resources": {name: resource_summary(samples, name, args.warmup_seconds)
                            for name in ["app", "child"]},
              "limitations": ["Flushed child writes and OS process metrics are progress proxies.",
                  "Byte counters count UTF-8 fixture payload before any ConPTY transformation.",
                  "No GPU frame, visual correctness, IME, or physical input-to-display latency measurement.",
                  "CPU percent uses one logical core as 100%; multi-threaded values can exceed 100%.",
                  "Memory trends are observations, not a leak verdict; desktop load is not isolated.",
                  "Successful completion leaves the dedicated GUI open; close it after inspection."]}
    atomic_json(directory / "report.json", report)
    print(json.dumps(report, ensure_ascii=False), flush=True)
    return 0 if completed else 1


def main(argv=None):
    args = parse_args(argv)
    try:
        return run_child(args) if args.child else run_monitor(args)
    except OSError as error:
        print(f"soak: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
