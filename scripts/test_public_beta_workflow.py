from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "public-beta.yml"


class PublicBetaWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        return WORKFLOW.read_text(encoding="utf-8")

    def test_version_tag_push_automatically_creates_only_a_draft(self) -> None:
        value = self.workflow()
        self.assertIn('  push:\n    tags:\n      - "v*"', value)
        self.assertIn("RELEASE_TAG: ${{ github.event_name == 'push'", value)
        self.assertIn("ref: ${{ env.RELEASE_TAG }}", value)
        self.assertIn("--draft", value)
        self.assertIn("--prerelease", value)

    def test_manual_dispatch_remains_a_recovery_path(self) -> None:
        value = self.workflow()
        self.assertIn("  workflow_dispatch:", value)
        self.assertIn("release_tag:", value)


if __name__ == "__main__":
    unittest.main()
