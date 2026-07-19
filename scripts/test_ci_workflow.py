from __future__ import annotations

import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
MAC_CONFIG = ROOT / "apps" / "desktop" / "src-tauri" / "tauri.macos.conf.json"


class CiWorkflowTests(unittest.TestCase):
    def workflow(self) -> str:
        return WORKFLOW.read_text(encoding="utf-8")

    def test_macos_packaging_covers_both_current_runner_architectures(self) -> None:
        value = self.workflow()
        self.assertIn("tauri-build-macos:", value)
        self.assertIn("runner: macos-latest", value)
        self.assertIn("arch: aarch64", value)
        self.assertIn("runner: macos-15-intel", value)
        self.assertIn("arch: x86_64", value)
        self.assertIn("runs-on: ${{ matrix.runner }}", value)
        self.assertIn("name: macos-dmg-${{ matrix.arch }}", value)
        self.assertIn("name: macos-app-${{ matrix.arch }}", value)
        self.assertNotIn("macos-13", value)

    def test_macos_build_uses_the_source_only_bundle_config(self) -> None:
        value = self.workflow()
        config = json.loads(MAC_CONFIG.read_text(encoding="utf-8"))
        self.assertIn("pnpm tauri build --config src-tauri/tauri.macos.conf.json", value)
        self.assertEqual(config["bundle"]["targets"], ["app", "dmg"])
        self.assertEqual(config["bundle"]["resources"], [])

    def test_windows_packaging_job_remains_present(self) -> None:
        value = self.workflow()
        self.assertIn("name: Source-only Windows Tauri packaging", value)
        self.assertIn("runs-on: windows-latest", value)
        self.assertIn("name: windows-installers", value)


if __name__ == "__main__":
    unittest.main()
