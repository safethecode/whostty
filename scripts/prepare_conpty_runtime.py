"""Stage Microsoft's matched x64 ConPTY runtime, without changing Windows itself."""

import argparse
import hashlib
import io
import urllib.request
import zipfile
from pathlib import Path


VERSION = "1.24.261001001"
PACKAGE_URL = (
    "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/"
    f"{VERSION}/microsoft.windows.console.conpty.{VERSION}.nupkg"
)
ARCHIVE_SHA256 = "4d6aaddc1d2385c9f5897df28f33879f699f8f2783315d5204cf3d8c3616ac5f"
RUNTIME_MEMBERS = {
    "conpty.dll": "runtimes/win-x64/native/conpty.dll",
    "OpenConsole.exe": "build/native/runtimes/x64/OpenConsole.exe",
}
RUNTIME_HASHES = {
    "conpty.dll": "5afea6c9480e2c7dfd195376dd10996b0ae25cdc6f014e6247fd8aedc8c34852",
    "OpenConsole.exe": "6f8e68dec4e8e5e15a54ecc8afa127b0e62139edc004ffbb4892125ff01d5737",
}
CONTEXT = Path(__file__).resolve().parent.parent / "context"


def runtime_files(archive: bytes, expected_hash: str) -> dict[str, bytes]:
    if hashlib.sha256(archive).hexdigest() != expected_hash:
        raise ValueError("ConPTY package checksum mismatch")
    with zipfile.ZipFile(io.BytesIO(archive)) as package:
        missing = [name for name, member in RUNTIME_MEMBERS.items() if member not in package.namelist()]
        if missing:
            raise ValueError(f"Missing ConPTY runtime files: {missing}")
        # Never extract paths supplied by the archive or another architecture.
        return {name: package.read(member) for name, member in RUNTIME_MEMBERS.items()}


def stage_runtime(archive: bytes, destination: Path) -> None:
    files = runtime_files(archive, ARCHIVE_SHA256)
    notices = (
        f"Microsoft.Windows.Console.ConPTY {VERSION}\nSource: {PACKAGE_URL}\n"
        f"Archive SHA256: {ARCHIVE_SHA256}\n\n"
        + (CONTEXT / "conpty-LICENSE.txt").read_text(encoding="utf-8")
        + "\n\nUpstream third-party notices (v1.24.12741.0):\n\n"
        + (CONTEXT / "conpty-NOTICE.txt").read_text(encoding="utf-8")
    )
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in files.items():
        (destination / name).write_bytes(data)
    (destination / "CONPTY_LICENSES.txt").write_text(notices, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--archive", type=Path, help="Use an already downloaded, hash-verified NuGet archive")
    args = parser.parse_args()
    if args.archive:
        archive = args.archive.read_bytes()
    else:
        with urllib.request.urlopen(PACKAGE_URL, timeout=60) as response:
            archive = response.read(16 * 1024 * 1024)
    stage_runtime(archive, args.destination)
    print(f"Staged Microsoft ConPTY {VERSION} x64 runtime and notices in {args.destination}")
