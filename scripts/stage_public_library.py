from __future__ import annotations

import hashlib
import json
import shutil
import sys
import zipfile
from pathlib import Path, PurePosixPath


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def stage(config_path: Path, archive: Path, destination: Path) -> None:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    expected = config.get("library_asset_sha256", "")
    if len(expected) != 64 or any(character not in "0123456789abcdef" for character in expected):
        raise ValueError("approved library SHA-256 is not pinned")
    if archive.name != config.get("library_asset_name") or sha256(archive) != expected:
        raise ValueError("library release asset does not match its pinned identity")
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    seen: set[str] = set()
    with zipfile.ZipFile(archive) as package:
        for info in package.infolist():
            path = PurePosixPath(info.filename.replace("\\", "/"))
            if (
                path.is_absolute()
                or ".." in path.parts
                or not path.parts
                or info.is_dir()
                or info.filename in seen
                or (info.external_attr >> 16) & 0o170000 == 0o120000
            ):
                raise ValueError("library release archive is not a closed set of plain files")
            seen.add(info.filename)
            target = destination.joinpath(*path.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            with package.open(info) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output, 1024 * 1024)


def main() -> int:
    if len(sys.argv) != 4:
        raise SystemExit("usage: stage_public_library.py CONFIG ARCHIVE DESTINATION")
    stage(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
