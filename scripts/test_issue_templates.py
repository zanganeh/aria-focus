"""Small repository hygiene checks for the public GitHub issue forms."""

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TEMPLATE_DIR = ROOT / ".github" / "ISSUE_TEMPLATE"


class IssueTemplateTests(unittest.TestCase):
    def assert_file_contains(self, filename: str, *needles: str) -> None:
        path = TEMPLATE_DIR / filename
        self.assertTrue(path.is_file(), f"missing issue-template file: {path}")
        text = path.read_text(encoding="utf-8")
        for needle in needles:
            with self.subTest(filename=filename, needle=needle):
                self.assertIn(needle, text)

    def test_bug_report_requires_reproduction_context(self) -> None:
        self.assert_file_contains(
            "bug_report.yml",
            "id: app-version",
            "id: windows-version",
            "id: reproduction",
            "id: expected",
            "id: actual",
            "id: logs",
            "id: music-studio-hardware",
        )

    def test_feature_request_captures_product_constraints(self) -> None:
        self.assert_file_contains(
            "feature_request.yml",
            "id: problem",
            "id: solution",
            "id: alternatives",
            "id: offline",
            "id: privacy",
        )

    def test_config_disables_blank_issues_and_routes_security(self) -> None:
        self.assert_file_contains(
            "config.yml",
            "blank_issues_enabled: false",
            "SECURITY.md",
        )


if __name__ == "__main__":
    unittest.main()
