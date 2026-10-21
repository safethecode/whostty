#!/usr/bin/env python3
"""Gate the fixed DX12 CI fixture; these are regression ceilings, not performance targets."""
import argparse
import json
from pathlib import Path

MIB = 1024 ** 2


def verify(directory):
    report = json.loads((directory / "report.json").read_text(encoding="utf-8"))
    if not (report["completed"] and report["fixture_exit_code"] == 0
            and report["app_alive_at_completion"] and report["fixture"]["lines"] >= 12000):
        raise ValueError("The complete native output fixture must pass before checking memory")
    if report["gpu_memory_mode"] != "compact" or report["scrollback_lines"] not in (0, 10000):
        raise ValueError("Expected the default compact configuration with 0 or 10,000 history lines")
    private = report["resources"]["app"]["memory"]["private_bytes"]["late_median_bytes"]
    if private > 128 * MIB:
        raise ValueError(f"Late PrivateUsage {private / MIB:.2f}MiB exceeds the 128MiB CI ceiling")
    entries = [json.loads(line) for line in (directory / "allocator.jsonl").read_text(encoding="utf-8").splitlines()]
    if len(entries) < 20 or not all(entry["gpu_allocator"] is not None for entry in entries):
        raise ValueError("Expected at least 20 supported DX12 allocator samples")
    if max(entry["gpu_allocator"]["reserved_bytes"] for entry in entries) > 64 * MIB:
        raise ValueError("DX12 reservations exceeded the 64MiB fixed-fixture ceiling")
    if max(entry["shaping"]["atlas_rgba_bytes"] for entry in entries) > 4 * MIB:
        raise ValueError("Small glyph fixture exceeded its 4MiB paged atlas ceiling")
    if not any(entry["shaping"]["glyph_entries"] > 0 for entry in entries):
        raise ValueError("No shaped glyphs were exercised")
    return {"directory": str(directory), "late_private_mib": private / MIB,
            "peak_gpu_reserved_mib": max(entry["gpu_allocator"]["reserved_bytes"] for entry in entries) / MIB,
            "peak_atlas_mib": max(entry["shaping"]["atlas_rgba_bytes"] for entry in entries) / MIB}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, nargs="+")
    args = parser.parse_args()
    for directory in args.directory:
        print(json.dumps(verify(directory)))
