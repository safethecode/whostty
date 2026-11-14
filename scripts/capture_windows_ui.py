"""Capture the real application through its opt-in GPU readback, never desktop pixels."""
import argparse
import binascii
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib


def capture(executable, output):
    output.mkdir(parents=True, exist_ok=False)
    image = (output / "client.ppm").resolve()
    with tempfile.TemporaryDirectory(prefix="whostty-ui-") as temporary:
        temporary = Path(temporary)
        config = temporary / "config.toml"
        config.write_text("font_size = 15.0\nscrollback = 10000\n", encoding="utf-8")
        fixture = temporary / "preview.py"
        fixture.write_text("import time\nprint('Whostty toolbar layout preview', flush=True)\ntime.sleep(30)\n", encoding="utf-8")
        environment = dict(os.environ, WHOSTTY_UI_SNAPSHOT=str(image))
        process = subprocess.Popen([str(executable), "--config", str(config), "-e", sys.executable, "-u", str(fixture)],
                                   env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            stdout, stderr = process.communicate(timeout=25)
        except subprocess.TimeoutExpired:
            subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=5)
            process.wait(timeout=5)
            raise
        (output / "app.log").write_bytes(stdout + stderr)
        if process.returncode:
            raise RuntimeError(f"UI capture exited with {process.returncode}")
    # Validate the actual output even if a failed capture run exited normally.
    data = image.read_bytes()
    magic, dimensions, maximum, pixels = data.split(b"\n", 3)
    width, height = map(int, dimensions.split())
    if magic != b"P6" or maximum != b"255" or not 480 <= width <= 8192 or not 300 <= height <= 8192:
        raise ValueError("Invalid UI snapshot header")
    if len(pixels) != width * height * 3:
        raise ValueError("Incomplete UI snapshot pixels")
    header = pixels[:width * min(height, 100) * 3]
    selected = sum(header[index:index + 3] == bytes((49, 72, 63)) for index in range(0, len(header), 3))
    if pixels[:3] != bytes((25, 30, 36)) or selected < 100:
        raise ValueError("Snapshot did not paint the dark toolbar and selected tab")

    def chunk(kind, content):
        return struct.pack("!I", len(content)) + kind + content + struct.pack("!I", binascii.crc32(kind + content))

    rows = b"".join(b"\0" + pixels[row * width * 3:(row + 1) * width * 3] for row in range(height))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!2I5B", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    (output / "client.png").write_bytes(png)
    report = {"width": width, "height": height, "selected_header_pixels": selected,
              "binary_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "png_sha256": hashlib.sha256(png).hexdigest(),
              "scope": "Actual Whostty client GPU readback with an isolated Python fixture; excludes OS window chrome, physical DPI transitions and physical input latency."}
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    capture(arguments.exe.resolve(strict=True), arguments.output)
