import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from replace_openrouter_item import replace_item


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def manifest_item(item_id: str, title: str, activity: str, audio: str, cover: str, root: Path):
    audio_bytes = (root / audio).read_bytes()
    cover_bytes = (root / cover).read_bytes()
    return {
        "id": item_id,
        "title": title,
        "activity_suitability": [
            {"activity": name, "suitability": 0.95 if name == activity else 0.0}
            for name in ["deep_work", "motivation", "creativity", "learning", "light_work"]
        ],
        "provenance": {
            "source": "test source",
            "licence_id": "test-licence",
            "licence_url": "https://example.test/terms",
            "generator": {
                "provider": "OpenRouter",
                "model": "test/audio",
                "model_version": "test/audio",
                "prompt": "instrumental focus music",
            },
        },
        "analysis": {"hard_rejections": [], "duration_seconds": 180},
        "variants": [
            {
                "id": "source",
                "asset": {
                    "path": audio,
                    "sha256": sha256(audio_bytes),
                    "bytes": len(audio_bytes),
                    "codec": "flac",
                },
            }
        ],
        "human_qa": {"status": "draft", "reviews": []},
        "cover": {
            "path": cover,
            "sha256": sha256(cover_bytes),
            "bytes": len(cover_bytes),
            "format": "png",
        },
    }


def write_pack(root: Path, item: dict, pack_id: str) -> None:
    (root / "assets").mkdir(parents=True, exist_ok=True)
    (root / "manifest.json").write_text(
        json.dumps(
            {
                "format": "aria-focus-library",
                "format_version": 2,
                "pack": {"id": pack_id, "version": "draft"},
                "taxonomy": {},
                "items": [item],
            }
        ),
        encoding="utf-8",
    )


class ReplaceOpenRouterItemTests(unittest.TestCase):
    def test_replaces_one_item_without_mutating_base_pack(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / "base"
            replacement = root / "replacement"
            output = root / "output"

            base_audio = "assets/base.flac"
            base_cover = "assets/base.png"
            replacement_audio = "assets/replacement.flac"
            replacement_cover = "assets/replacement.png"
            (base / "assets").mkdir(parents=True)
            (replacement / "assets").mkdir(parents=True)
            (base / base_audio).write_bytes(b"audio-base")
            (base / base_cover).write_bytes(b"cover-base")
            (replacement / replacement_audio).write_bytes(b"audio-replacement")
            (replacement / replacement_cover).write_bytes(b"cover-replacement")

            base_item = manifest_item("base-item", "Stable title", "learning", base_audio, base_cover, base)
            replacement_item = manifest_item(
                "replacement-item", "Replacement title", "learning", replacement_audio, replacement_cover, replacement
            )
            write_pack(base, base_item, "base-pack")
            write_pack(replacement, replacement_item, "replacement-pack")

            replace_item(base, replacement, output, "base-item", "replacement-item", "2.0.0-draft.2")

            self.assertEqual((base / base_audio).read_bytes(), b"audio-base")
            self.assertEqual((output / base_audio).read_bytes(), b"audio-replacement")
            self.assertEqual((output / base_cover).read_bytes(), b"cover-replacement")
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["pack"]["version"], "2.0.0-draft.2")
            self.assertEqual(manifest["items"][0]["id"], "base-item")
            self.assertEqual(manifest["items"][0]["title"], "Stable title")
            self.assertIn("replacement-pack", manifest["items"][0]["provenance"]["source"])

            empty_base = root / "empty-base"
            (empty_base / "assets").mkdir(parents=True)
            (empty_base / "manifest.json").write_text(
                json.dumps(
                    {
                        "format": "aria-focus-library",
                        "format_version": 2,
                        "pack": {"id": "base-pack", "version": "draft"},
                        "taxonomy": {"genres": [], "moods": []},
                        "items": [],
                    }
                ),
                encoding="utf-8",
            )
            inserted = root / "inserted"
            replace_item(
                empty_base,
                replacement,
                inserted,
                "base-item",
                "replacement-item",
                "2.0.0-draft.2",
                "Inserted title",
            )
            inserted_manifest = json.loads((inserted / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(len(inserted_manifest["items"]), 1)
            self.assertEqual(inserted_manifest["items"][0]["id"], "base-item")
            self.assertEqual(inserted_manifest["items"][0]["title"], "Inserted title")


if __name__ == "__main__":
    unittest.main()
