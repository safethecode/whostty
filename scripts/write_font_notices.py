"""Collect the actual locked egui font notices without a fontTools dependency."""

import argparse
import hashlib
import json
import struct
import subprocess
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
FONT_NOTICES = {
    "Hack-Regular.ttf": "Hack-Regular.txt",
    "NotoEmoji-Regular.ttf": "OFL.txt",
    "Ubuntu-Light.ttf": "UFL.txt",
    "emoji-icon-font.ttf": "emoji-icon-font-mit-license.txt",
}
FIELDS = {0: "Copyright", 13: "License metadata", 14: "License URL"}


def name_metadata(data: bytes) -> dict[int, str]:
    """Read SFNT Unicode/Mac name fields, preferring the English Windows record."""
    if len(data) < 12 or data[:4] not in (b"\x00\x01\x00\x00", b"OTTO", b"true"):
        raise ValueError("Unsupported or truncated font header")
    count = struct.unpack_from(">H", data, 4)[0]
    if 12 + count * 16 > len(data):
        raise ValueError("Truncated font directory")
    table = None
    for index in range(count):
        tag, _, offset, length = struct.unpack_from(">4sIII", data, 12 + index * 16)
        if offset + length > len(data):
            raise ValueError("Font table is outside the file")
        if tag == b"name":
            table = data[offset:offset + length]
    if table is None or len(table) < 6:
        raise ValueError("Missing font name table")
    format_version, count, storage = struct.unpack_from(">HHH", table)
    if format_version not in (0, 1):
        raise ValueError("Unsupported font name table format")
    if 6 + count * 12 > len(table) or storage < 6 + count * 12 or storage > len(table):
        raise ValueError("Invalid font name records")
    candidates = {}
    for index in range(count):
        platform, encoding, language, name, length, offset = struct.unpack_from(">HHHHHH", table, 6 + index * 12)
        if storage + offset + length > len(table):
            raise ValueError("Font name string is outside its table")
        if name not in FIELDS or platform not in (0, 1, 3):
            continue
        if platform == 1 and encoding != 0:
            continue  # Non-Roman legacy Macintosh encodings require a different decoder.
        raw = table[storage + offset:storage + offset + length]
        try:
            text = raw.decode("mac_roman" if platform == 1 else "utf-16-be").strip()
        except UnicodeError as error:
            raise ValueError("Invalid font name encoding") from error
        if not text:
            continue
        priority = 0 if (platform, language) == (3, 0x409) else 1 if platform == 0 else 2 if (platform, language) == (1, 0) else 3
        candidates.setdefault(name, []).append((priority, platform, language, text))
    return {name: min(records)[3] for name, records in candidates.items()}


def render_notices(directory: Path, version: str) -> str:
    inventory = {path.name for path in directory.iterdir() if path.suffix.lower() in (".ttf", ".otf", ".ttc", ".woff", ".woff2")}
    if inventory != set(FONT_NOTICES):
        raise ValueError(f"Embedded font inventory changed; review notice mapping: {sorted(inventory)}")
    parts = [
        "Whostty embedded font notices",
        f"Fonts supplied by epaint_default_fonts {version} (locked Cargo dependency).",
        f"Source: https://crates.io/crates/epaint_default_fonts/{version}",
        "These font terms are separate from Whostty's MIT license.",
    ]
    for filename, notice in sorted(FONT_NOTICES.items()):
        data = (directory / filename).read_bytes()
        metadata = name_metadata(data)
        # The icon font stores attribution only in its accompanying MIT notice.
        if filename != "emoji-icon-font.ttf" and not metadata.get(0):
            raise ValueError(f"Missing copyright metadata: {filename}")
        parts.extend(["", "=" * 72, filename, f"SHA256: {hashlib.sha256(data).hexdigest()}"])
        for field, label in FIELDS.items():
            if field in metadata:
                parts.extend([f"{label}:", metadata[field]])
        text = (directory / notice).read_text(encoding="utf-8").strip()
        if not text:
            raise ValueError(f"Empty font notice: {notice}")
        parts.extend([f"Upstream notice ({notice}):", text])
    return "\n\n".join(parts) + "\n"


def locked_font_package() -> tuple[Path, str]:
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version=1", "--filter-platform", "x86_64-pc-windows-msvc"],
        cwd=PROJECT, check=True, stdout=subprocess.PIPE, text=True, encoding="utf-8",
    )
    packages = [package for package in json.loads(result.stdout)["packages"] if package["name"] == "epaint_default_fonts"]
    if len(packages) != 1:
        raise ValueError("Expected exactly one epaint_default_fonts dependency; review the resolved font inventory")
    package = packages[0]
    return Path(package["manifest_path"]).parent / "fonts", package["version"]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    directory, version = locked_font_package()
    text = render_notices(directory, version)
    with args.output.open("w", encoding="utf-8", newline="\n") as output:
        output.write(text)
    print(f"Collected notices for {len(FONT_NOTICES)} fonts from epaint_default_fonts {version}")


if __name__ == "__main__":
    main()
