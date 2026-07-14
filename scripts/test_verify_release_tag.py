from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from verify_release_tag import verify


class ReleaseTagTests(unittest.TestCase):
    def fixture(self) -> Path:
        root = Path(self.addCleanupDirectory())
        (root / "apps/desktop/src-tauri").mkdir(parents=True)
        (root / "release").mkdir()
        (root / "package.json").write_text(json.dumps({"version": "1.2.3-beta.4"}), encoding="utf-8")
        (root / "apps/desktop/package.json").write_text(
            json.dumps({"version": "1.2.3-beta.4"}), encoding="utf-8"
        )
        (root / "Cargo.toml").write_text(
            '[workspace.package]\nversion = "1.2.3-beta.4"\n', encoding="utf-8"
        )
        (root / "apps/desktop/src-tauri/tauri.conf.json").write_text(
            json.dumps({"version": "1.2.3"}), encoding="utf-8"
        )
        (root / "release/public-beta-assets.json").write_text(
            json.dumps(
                {
                    "library_release_tag": "aria-focus-library-v1",
                    "library_asset_name": "aria-focus-library-v1.zip",
                    "library_asset_sha256": "a" * 64,
                }
            ),
            encoding="utf-8",
        )
        return root

    def addCleanupDirectory(self) -> str:
        directory = tempfile.mkdtemp()
        self.addCleanup(lambda: __import__("shutil").rmtree(directory, ignore_errors=True))
        return directory

    def test_accepts_consistent_prerelease_identity(self) -> None:
        self.assertEqual(verify(self.fixture(), "v1.2.3-beta.4"), "1.2.3-beta.4")

    def test_rejects_tag_version_drift(self) -> None:
        with self.assertRaisesRegex(ValueError, "does not match"):
            verify(self.fixture(), "v1.2.4-beta.4")

    def test_rejects_unpinned_library_hash(self) -> None:
        root = self.fixture()
        path = root / "release/public-beta-assets.json"
        value = json.loads(path.read_text(encoding="utf-8"))
        value["library_asset_sha256"] = "REPLACE_AFTER_LIBRARY_APPROVAL"
        path.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            verify(root, "v1.2.3-beta.4")


if __name__ == "__main__":
    unittest.main()
