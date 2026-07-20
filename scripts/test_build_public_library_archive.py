import tempfile
import unittest
from pathlib import Path

import build_public_library_archive as subject


class PublicLibraryArchiveTests(unittest.TestCase):
    def test_declared_paths_are_sorted_and_include_cover_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "manifest.json").write_text(
                '{"items":[{"variants":[{"asset":{"path":"assets/audio.flac"}}],'
                '"cover":{"path":"assets/cover.png"}}]}',
                encoding="utf-8",
            )
            self.assertEqual(
                subject.declared_paths(root),
                ["assets/audio.flac", "assets/cover.png", "manifest.json"],
            )

    def test_build_is_deterministic_for_the_same_verified_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pack"
            output = Path(directory) / "out.zip"
            root.mkdir()
            (root / "manifest.json").write_text('{"items":[]}', encoding="utf-8")

            original_verify = subject.verify
            subject.verify = lambda _: None
            try:
                first = subject.build(root, output)
                output.unlink()
                second = subject.build(root, output)
            finally:
                subject.verify = original_verify
            self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()
