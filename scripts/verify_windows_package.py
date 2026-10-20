"""Validate the portable artifact without executing Windows code on the host."""

import hashlib
import struct
import sys
import zipfile
from pathlib import Path

from prepare_conpty_runtime import CONTEXT, RUNTIME_HASHES, VERSION


def verify(path: Path) -> None:
    expected = path.with_suffix(".zip.sha256").read_text().split()[0]
    assert hashlib.sha256(path.read_bytes()).hexdigest() == expected, "ZIP checksum mismatch"
    with zipfile.ZipFile(path) as package:
        expected_files = ["CONPTY_LICENSES.txt", "EMBEDDED_FONT_LICENSES.txt", "LICENSE", "THIRD_PARTY_LICENSES.html", "whostty.exe", *RUNTIME_HASHES]
        assert sorted(package.namelist()) == sorted(expected_files), "Missing or unexpected package files (including ConPTY runtime pair)"
        assert package.read("LICENSE").startswith(b"MIT License")
        notices = package.read("THIRD_PARTY_LICENSES.html")
        assert b"alacritty_terminal" in notices and b"portable-pty" in notices
        fonts = package.read("EMBEDDED_FONT_LICENSES.txt").decode("utf-8")
        for attribution in ("Source Foundry", "Bitstream", "Canonical", "Google", "John Slegers"):
            assert attribution in fonts, f"Missing embedded font attribution: {attribution}"
        for font in ("Hack-Regular.ttf", "NotoEmoji-Regular.ttf", "Ubuntu-Light.ttf", "emoji-icon-font.ttf"):
            assert font in fonts, f"Missing embedded font: {font}"
        assert fonts.count("SHA256:") == 4, "Expected hashes for the four embedded fonts"
        conpty_notices = package.read("CONPTY_LICENSES.txt").decode("utf-8")
        assert f"Microsoft.Windows.Console.ConPTY {VERSION}" in conpty_notices
        for notice in ("conpty-LICENSE.txt", "conpty-NOTICE.txt"):
            assert (CONTEXT / notice).read_text(encoding="utf-8") in conpty_notices, f"Missing {notice}"
        for name, digest in RUNTIME_HASHES.items():
            data = package.read(name)
            assert hashlib.sha256(data).hexdigest() == digest, f"ConPTY runtime checksum mismatch: {name}"
            verify_pe(data)
        exe = package.read("whostty.exe")
    verify_pe(exe, gui=True, dpi=True)
    print("Verified ZIP checksum, dependency/font/ConPTY notices, matched x64 ConPTY runtime, GUI executable, DPI manifest, and static VC++ runtime.")


def verify_pe(exe: bytes, *, gui: bool = False, dpi: bool = False) -> None:
    assert exe[:2] == b"MZ", "Missing DOS header"
    pe = struct.unpack_from("<I", exe, 60)[0]
    assert exe[pe:pe + 4] == b"PE\0\0", "Missing PE header"
    machine, section_count = struct.unpack_from("<HH", exe, pe + 4)
    optional_size = struct.unpack_from("<H", exe, pe + 20)[0]
    optional = pe + 24
    assert machine == 0x8664, "Expected Windows x64"
    assert struct.unpack_from("<H", exe, optional)[0] == 0x20B, "Expected PE32+"
    if gui:
        assert struct.unpack_from("<H", exe, optional + 68)[0] == 2, "Expected GUI subsystem"
    if dpi:
        assert b"PerMonitorV2" in exe, "DPI manifest is missing"
    sections = optional + optional_size

    def offset(rva: int) -> int:
        for index in range(section_count):
            virtual_size, address, raw_size, raw_offset = struct.unpack_from("<IIII", exe, sections + index * 40 + 8)
            if address <= rva < address + max(virtual_size, raw_size):
                return raw_offset + rva - address
        raise AssertionError(f"Unmapped PE address: {rva}")

    descriptor = offset(struct.unpack_from("<I", exe, optional + 120)[0])
    libraries = []
    while any(exe[descriptor:descriptor + 20]):
        name = offset(struct.unpack_from("<I", exe, descriptor + 12)[0])
        libraries.append(exe[name:exe.index(0, name)].decode().lower())
        descriptor += 20
    external_crt = [name for name in libraries if name.startswith(("vcruntime", "msvcp", "msvcr"))]
    assert not external_crt, f"Portable package depends on VC++ redistributable: {external_crt}"


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
