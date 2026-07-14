from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path

TAG = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


def load_json(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def load_toml(path: Path) -> dict:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def verify(root: Path, tag: str) -> str:
    if not TAG.fullmatch(tag):
        raise ValueError("release tag must be a canonical vMAJOR.MINOR.PATCH tag with an optional prerelease")

    package = load_json(root / "package.json")
    desktop = load_json(root / "apps/desktop/package.json")
    cargo = load_toml(root / "Cargo.toml")
    tauri = load_json(root / "apps/desktop/src-tauri/tauri.conf.json")
    assets = load_json(root / "release/public-beta-assets.json")

    version = package.get("version")
    expected_tag = f"v{version}"
    if tag != expected_tag:
        raise ValueError(f"release tag {tag!r} does not match package version {expected_tag!r}")
    if desktop.get("version") != version:
        raise ValueError("desktop package version differs from the workspace package version")
    if cargo.get("workspace", {}).get("package", {}).get("version") != version:
        raise ValueError("Cargo workspace version differs from the package version")

    numeric_version = str(version).split("-", 1)[0]
    if tauri.get("version") != numeric_version:
        raise ValueError("Tauri installer version must equal the numeric core of the package version")

    library_tag = assets.get("library_release_tag")
    library_name = assets.get("library_asset_name")
    library_hash = assets.get("library_asset_sha256")
    if not isinstance(library_tag, str) or not library_tag or library_tag == tag:
        raise ValueError("reviewed library must use a nonempty, separate release tag")
    if not isinstance(library_name, str) or not library_name.endswith(".zip"):
        raise ValueError("reviewed library asset name must be a pinned .zip filename")
    if not isinstance(library_hash, str) or not SHA256.fullmatch(library_hash):
        raise ValueError("reviewed library SHA-256 is not pinned as 64 lowercase hex characters")

    return version


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify an Aria Focus release tag and pinned inputs")
    parser.add_argument("tag")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    arguments = parser.parse_args()
    version = verify(arguments.root.resolve(), arguments.tag)
    print(f"release identity: ok ({version})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
