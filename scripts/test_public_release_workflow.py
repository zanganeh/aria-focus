from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "public-release.yml"


class PublicReleaseWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        return WORKFLOW.read_text(encoding="utf-8")

    def test_version_tag_push_automatically_creates_only_a_draft(self) -> None:
        value = self.workflow()
        self.assertIn('  push:\n    tags:\n      - "v*"', value)
        self.assertIn("RELEASE_TAG: ${{ github.event_name == 'push'", value)
        self.assertIn("SOURCE_REF: ${{ github.event_name == 'push'", value)
        self.assertIn("ref: ${{ env.SOURCE_REF }}", value)
        self.assertIn("--draft", value)
        # Stable releases must not be published as GitHub prereleases.
        self.assertNotIn("--prerelease", value)

    def test_manual_dispatch_creates_the_tag_after_gates(self) -> None:
        value = self.workflow()
        self.assertIn("  workflow_dispatch:", value)
        self.assertIn("release_tag:", value)
        self.assertIn("source_ref:", value)
        self.assertIn("name: Create release tag from Actions", value)
        self.assertIn("github.event_name == 'workflow_dispatch'", value)

    def test_workflow_is_named_for_stable_release(self) -> None:
        self.assertIn("name: Signed public release", self.workflow())


if __name__ == "__main__":
    unittest.main()
