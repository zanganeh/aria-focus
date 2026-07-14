import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import verify_public_library as gate


class PublicLibraryGateTests(unittest.TestCase):
    def test_requires_exact_reviewed_twenty_per_activity_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            items = []
            for activity in gate.ACTIVITIES:
                for index in range(20):
                    payload = f"{activity}-{index}".encode()
                    relative = f"assets/{activity}-{index}.flac"
                    path = root / relative
                    path.parent.mkdir(exist_ok=True)
                    path.write_bytes(payload)
                    items.append(
                        {
                            "id": f"{activity}-{index}",
                            "activity_suitability": [
                                {"activity": activity, "suitability": 1.0}
                            ],
                            "provenance": {
                                "licence_id": "approved-output",
                                "licence_url": "https://example.invalid/licence",
                                "contains_lyrics": False,
                                "contains_speech": False,
                            },
                            "human_qa": {
                                "status": "approved",
                                "reviews": [{"reviewer": "one"}, {"reviewer": "two"}],
                            },
                            "variants": [
                                {
                                    "asset": {
                                        "path": relative,
                                        "bytes": len(payload),
                                        "sha256": hashlib.sha256(payload).hexdigest(),
                                    }
                                }
                            ],
                        }
                    )
            (root / "manifest.json").write_text(
                json.dumps({"pack": {"title": "Aria Focus Library"}, "items": items}),
                encoding="utf-8",
            )
            gate.verify(root)
            items[0]["human_qa"]["status"] = "draft"
            (root / "manifest.json").write_text(
                json.dumps({"pack": {"title": "Aria Focus Library"}, "items": items}),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "two-reviewer approval"):
                gate.verify(root)


if __name__ == "__main__":
    unittest.main()
