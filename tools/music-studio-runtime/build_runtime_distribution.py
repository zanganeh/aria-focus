from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import tarfile
from pathlib import Path

MAX_PUBLIC_CHUNK_BYTES = 1_900_000_000


def safe_path(value: str) -> Path:
    path = Path(value).expanduser().resolve()
    if not path.is_absolute():
        raise argparse.ArgumentTypeError("path must resolve absolutely")
    return path


def load_json_strict(path: Path) -> dict:
    def pairs(values: list[tuple[str, object]]) -> dict:
        result: dict[str, object] = {}
        for key, value in values:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def private_key(path: Path):
    from cryptography.hazmat.primitives.serialization import load_pem_private_key

    return load_pem_private_key(path.read_bytes(), password=None)


def validate_package(source: Path, signing_key) -> dict:
    manifest_path = source / "package-manifest.json"
    signature_path = source / "package-manifest.sig"
    manifest = load_json_strict(manifest_path)
    canonical = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    if canonical != manifest_path.read_bytes():
        raise ValueError("package manifest is not canonical")
    signature = base64.b64decode(signature_path.read_text(encoding="ascii").strip(), validate=True)
    signing_key.public_key().verify(signature, canonical)
    expected = {"package-manifest.json", "package-manifest.sig"}
    total = 0
    for entry in manifest["files"]:
        relative = Path(entry["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("unsafe package path")
        path = source / "runtime" / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"missing package file: {relative}")
        if path.stat().st_size != entry["bytes"] or sha256(path) != entry["sha256"]:
            raise ValueError(f"package file differs: {relative}")
        total += entry["bytes"]
        expected.add((Path("runtime") / relative).as_posix())
    if total != manifest["required_bytes"]:
        raise ValueError("package required_bytes differs")
    actual = {
        path.relative_to(source).as_posix()
        for path in source.rglob("*")
        if path.is_file()
    }
    if actual != expected:
        raise ValueError("package root is not closed-world")
    return manifest


class ChunkWriter:
    def __init__(self, output: Path, limit: int):
        self.output = output
        self.limit = limit
        self.index = 0
        self.stream = None
        self.size = 0
        self.digest = None
        self.records: list[dict] = []

    def writable(self) -> bool:
        return True

    def _open(self) -> None:
        name = f"runtime-{self.index:03d}.part"
        self.stream = (self.output / name).open("xb")
        self.size = 0
        self.digest = hashlib.sha256()

    def _finish(self) -> None:
        if self.stream is None:
            return
        self.stream.flush()
        os.fsync(self.stream.fileno())
        self.stream.close()
        name = f"runtime-{self.index:03d}.part"
        self.records.append(
            {
                "index": self.index,
                "file_name": name,
                "bytes": self.size,
                "sha256": self.digest.hexdigest(),
            }
        )
        self.index += 1
        self.stream = None

    def write(self, data: bytes) -> int:
        view = memoryview(data)
        consumed = 0
        while view:
            if self.stream is None:
                self._open()
            remaining = self.limit - self.size
            part = view[:remaining]
            self.stream.write(part)
            self.digest.update(part)
            written = len(part)
            self.size += written
            consumed += written
            view = view[written:]
            if self.size == self.limit:
                self._finish()
        return consumed

    def flush(self) -> None:
        if self.stream is not None:
            self.stream.flush()

    def close(self) -> None:
        self._finish()


def normalized(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    info.mode = 0o755 if info.isdir() else 0o644
    return info


def build(source: Path, output: Path, signing_key, chunk_bytes: int) -> dict:
    if output.exists():
        raise ValueError("output must not already exist")
    if not source.is_dir() or source.is_symlink():
        raise ValueError("source must be an ordinary package directory")
    if not 1024 <= chunk_bytes <= MAX_PUBLIC_CHUNK_BYTES:
        raise ValueError("chunk size is outside the supported range")
    package = validate_package(source, signing_key)
    output.mkdir(parents=True)
    writer = ChunkWriter(output, chunk_bytes)
    with tarfile.open(fileobj=writer, mode="w|", format=tarfile.GNU_FORMAT) as archive:
        for path in sorted(source.rglob("*"), key=lambda value: value.relative_to(source).as_posix()):
            if path.is_symlink():
                raise ValueError("package may not contain links")
            archive.add(
                path,
                arcname=path.relative_to(source).as_posix(),
                recursive=False,
                filter=normalized,
            )
    writer.close()
    manifest = {
        "format": 1,
        "runtime_version": package["runtime_version"],
        "package_manifest_sha256": sha256(source / "package-manifest.json"),
        "required_bytes": package["required_bytes"],
        "download_bytes": sum(record["bytes"] for record in writer.records),
        "chunks": writer.records,
    }
    canonical = json.dumps(manifest, separators=(",", ":")).encode()
    signature = signing_key.sign(canonical)
    (output / "runtime-distribution.json").write_bytes(canonical)
    (output / "runtime-distribution.sig").write_text(
        base64.b64encode(signature).decode("ascii") + "\n", encoding="ascii"
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=safe_path)
    parser.add_argument("--output", required=True, type=safe_path)
    parser.add_argument(
        "--private-key",
        type=safe_path,
        default=os.getenv("MUSIC_STUDIO_RUNTIME_SIGNING_KEY"),
    )
    parser.add_argument("--chunk-bytes", type=int, default=1_800_000_000)
    args = parser.parse_args()
    if args.private_key is None:
        parser.error("--private-key or MUSIC_STUDIO_RUNTIME_SIGNING_KEY is required")
    manifest = build(args.source, args.output, private_key(args.private_key), args.chunk_bytes)
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
