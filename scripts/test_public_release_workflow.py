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
        self.assertIn("publish_release:", value)
        self.assertIn("name: Create release tag from Actions after signing", value)
        self.assertIn("github.event_name == 'workflow_dispatch'", value)
        self.assertLess(
            value.index("name: Verify signed outputs and checksums"),
            value.index("name: Create release tag from Actions after signing"),
        )

    def test_workflow_is_named_for_stable_release(self) -> None:
        self.assertIn("name: Signed public release", self.workflow())

    def test_release_includes_signed_macos_assets_for_both_architectures(self) -> None:
        value = self.workflow()
        self.assertIn("build-macos-release-assets:", value)
        self.assertIn("needs: build-sign-publish", value)
        self.assertIn("arch: aarch64", value)
        self.assertIn("arch: x86_64", value)
        self.assertIn("tauri.macos.release.conf.json", value)
        self.assertIn("xcrun stapler validate", value)
        self.assertIn("gh release upload \"$RELEASE_TAG\"", value)

    def test_release_requires_external_signing_gates(self) -> None:
        value = self.workflow()
        for name in (
            "SIGNPATH_API_TOKEN",
            "APPLE_CERTIFICATE",
            "APPLE_CERTIFICATE_PASSWORD",
            "APPLE_ID",
            "APPLE_PASSWORD",
            "APPLE_TEAM_ID",
        ):
            self.assertIn(name, value)
        self.assertIn("Validate protected release configuration", value)


if __name__ == "__main__":
    unittest.main()
