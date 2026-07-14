import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

import stage_public_library as subject


class StagePublicLibraryTests(unittest.TestCase):
    def test_requires_pinned_archive_and_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "library.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("manifest.json", "{}")
            config = root / "config.json"
            config.write_text(
                json.dumps(
                    {
                        "library_asset_name": archive.name,
                        "library_asset_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                    }
                ),
                encoding="utf-8",
            )
            destination = root / "out"
            subject.stage(config, archive, destination)
            self.assertEqual((destination / "manifest.json").read_text(), "{}")

            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("../outside", "bad")
            config.write_text(
                json.dumps(
                    {
                        "library_asset_name": archive.name,
                        "library_asset_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "closed set"):
                subject.stage(config, archive, destination)


if __name__ == "__main__":
    unittest.main()
