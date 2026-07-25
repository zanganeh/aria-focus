from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from review_openrouter_pack import ReviewError, apply_review, review_pack


def manifest() -> dict:
    return {
        "format": "adhdpack",
        "format_version": 1,
        "pack": {"title": "Aria Focus Library"},
        "items": [{"id": "track-01", "human_qa": {"status": "draft", "reviews": []}}],
    }


class ReviewPackTests(unittest.TestCase):
    def test_two_distinct_approvals_promote_item(self) -> None:
        value = manifest()
        apply_review(
            value,
            item_id="track-01",
            reviewer_id="reviewer-a",
            status="approved",
            notes="Listened to the full track; no distracting events.",
            representative_work_session=True,
            session_minutes=45,
            reviewed_at="2026-07-24T00:00:00Z",
        )
        self.assertEqual(value["items"][0]["human_qa"]["status"], "draft")
        apply_review(
            value,
            item_id="track-01",
            reviewer_id="reviewer-b",
            status="approved",
            notes="Second full listen confirms stable focus suitability.",
            representative_work_session=False,
            session_minutes=0,
            reviewed_at="2026-07-24T01:00:00Z",
        )
        self.assertEqual(value["items"][0]["human_qa"]["status"], "approved")

    def test_duplicate_reviewer_is_rejected(self) -> None:
        value = manifest()
        kwargs = dict(
            item_id="track-01",
            reviewer_id="reviewer-a",
            status="approved",
            notes="A sufficiently detailed listening review.",
            representative_work_session=False,
            session_minutes=0,
            reviewed_at="2026-07-24T00:00:00Z",
        )
        apply_review(value, **kwargs)
        with self.assertRaises(ReviewError):
            apply_review(value, **kwargs)

    def test_rejection_keeps_item_out_of_release(self) -> None:
        value = manifest()
        apply_review(
            value,
            item_id="track-01",
            reviewer_id="reviewer-a",
            status="rejected",
            notes="Long silent gap and distracting high-frequency artifact.",
            representative_work_session=False,
            session_minutes=0,
            reviewed_at="2026-07-24T00:00:00Z",
        )
        self.assertEqual(value["items"][0]["human_qa"]["status"], "rejected")

    def test_pack_write_is_atomic_and_round_trips(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "manifest.json").write_text(json.dumps(manifest()), encoding="utf-8")
            review_pack(
                root,
                item_id="track-01",
                reviewer_id="reviewer-a",
                status="approved",
                notes="Full listen completed with no release blockers.",
                representative_work_session=False,
                session_minutes=0,
                reviewed_at="2026-07-24T00:00:00Z",
            )
            saved = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(saved["items"][0]["human_qa"]["reviews"][0]["reviewer_id"], "reviewer-a")


if __name__ == "__main__":
    unittest.main()
