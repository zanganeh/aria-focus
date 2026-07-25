from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from derive_prompt_catalog import build_prompt, derive_catalog  # noqa: E402


class DerivePromptCatalogTests(unittest.TestCase):
    def test_catalog_keeps_cooccurrence_and_removes_private_fields(self) -> None:
        response = {
            "result": {
                "servings": [
                    {
                        "track": {
                            "name": "Private source title",
                            "id": "private-track-id",
                            "imageUrl": "https://example.invalid/art.webp",
                            "beatsPerMinute": 116,
                            "brightnessLevel": 0.65,
                            "complexityLevel": 0.2,
                            "mentalState": {"displayValue": "Focus"},
                            "tags": [
                                {"type": "activity", "value": "Creativity"},
                                {"type": "genre", "value": "Electronic"},
                                {"type": "subgenre", "value": "Electronica"},
                                {"type": "mood", "value": "Uplifting"},
                                {"type": "instrument", "value": "Processed Vocals"},
                                {"type": "instrument", "value": "Electric Keys"},
                            ],
                            "relatedActivities": [{"displayValue": "Motivation"}],
                            "variations": [{"style": "normal", "lengthInSeconds": 180}],
                        },
                        "trackVariation": {"neuralEffectLevel": 0.76, "tokenedUrl": "secret"},
                    }
                ]
            }
        }
        har = {
            "log": {
                "entries": [
                    {
                        "request": {
                            "postData": {
                                "text": json.dumps(
                                    {"genreNames": ["Electronic", "Atmospheric"], "neuralEffectLevels": ["High"]}
                                )
                            }
                        },
                        "response": {"content": {"text": json.dumps(response)}},
                    }
                ]
            }
        }

        with tempfile.TemporaryDirectory() as directory:
            har_path = Path(directory) / "session.har"
            har_path.write_text(json.dumps(har), encoding="utf-8")
            catalog = derive_catalog(har_path, "2026-07-22")

        self.assertEqual(catalog["profile_count"], 1)
        self.assertEqual(catalog["dimensions"]["selector_genres"], ["Atmospheric", "Electronic"])
        self.assertEqual(catalog["profiles"][0]["activities"], ["Creativity", "Motivation"])
        serialized = json.dumps(catalog)
        self.assertNotIn("Private source title", serialized)
        self.assertNotIn("tokenedUrl", serialized)
        self.assertNotIn("example.invalid", serialized)

    def test_prompt_turns_voice_like_tags_into_negative_constraints(self) -> None:
        profile = {
            "profile_id": "profile-001",
            "genres": ["Electronic"],
            "subgenres": ["Electronica"],
            "moods": ["Driving", "Uplifting"],
            "instruments": ["Arp Synth", "Processed Vocals", "Electric Keys"],
            "bpm": 116,
            "brightness": 0.65,
            "complexity": 0.2,
        }
        result = build_prompt(profile, activity="Motivation", avoid=("jazz",))

        self.assertIn("116 BPM", result["prompt"]["positive"])
        self.assertIn("under 10 seconds", result["prompt"]["positive"])
        self.assertNotIn("Processed Vocals", result["prompt"]["positive"])
        self.assertIn("Processed Vocals", result["selection"]["excluded_instruments"])
        self.assertIn("jazz", result["prompt"]["negative"])
        self.assertEqual(result["policy"]["intro_max_seconds"], 10)


if __name__ == "__main__":
    unittest.main()
