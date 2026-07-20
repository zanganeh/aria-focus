from __future__ import annotations

import argparse
import hashlib
import os
import tempfile
import zipfile
from pathlib import Path

from verify_public_library import load_strict, verify


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def declared_paths(root: Path) -> list[str]:
    manifest = load_strict(root / "manifest.json")
    paths = {"manifest.json"}
    for item in manifest["items"]:
        paths.update(variant["asset"]["path"].replace("\\", "/") for variant in item["variants"])
        if item.get("cover"):
            paths.add(item["cover"]["path"].replace("\\", "/"))
    return sorted(paths)


def build(root: Path, output: Path) -> str:
    root = root.resolve()
    output = output.resolve()
    verify(root)
    if output.parent == root or root in output.parents:
        raise ValueError("archive output must not be inside the library root")

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=output.parent, prefix=f".{output.name}.", suffix=".tmp", delete=False
    ) as temporary:
        temporary_path = Path(temporary.name)
    try:
        with zipfile.ZipFile(
            temporary_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
        ) as package:
            for relative in declared_paths(root):
                info = zipfile.ZipInfo(relative)
                info.date_time = (1980, 1, 1, 0, 0, 0)
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                package.writestr(info, (root / relative).read_bytes())
        os.replace(temporary_path, output)
    finally:
        temporary_path.unlink(missing_ok=True)
    return sha256(output)


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the pinned public library ZIP")
    parser.add_argument("--root", type=Path, default=Path("apps/desktop/src-tauri/private-beta-pack"))
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    digest = build(arguments.root, arguments.output)
    print(f"archive: {arguments.output.resolve()}")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
