from __future__ import annotations

import hashlib
import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("convert_library_to_opus", ROOT / "tools/music-generation/convert_library_to_opus.py")
assert SPEC and SPEC.loader
conversion = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(conversion)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class ConvertLibraryToOpusTests(unittest.TestCase):
    def make_source(self, root: Path) -> Path:
        source = root / "masters"
        (source / "assets").mkdir(parents=True)
        asset = source / "assets/one.flac"
        asset.write_bytes(b"fLaC-master-bytes")
        manifest = {
            "format": "adhdpack", "format_version": 1,
            "pack": {"id": "candidate", "version": "0.1.0"},
            "items": [{
                "id": "keep-this-id", "analysis": {"duration_seconds": 1.0},
                "variants": [{"id": "source", "asset": {"path": "assets/one.flac", "sha256": digest(asset), "bytes": asset.stat().st_size, "codec": "flac", "sample_rate_hz": 48000, "channels": 2, "bit_depth": 16}, "safe_regions": [{"kind": "loop", "start_seconds": 0.0, "end_seconds": 1.0}], "stimulation_available": ["off"]}],
            }],
        }
        (source / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        return source

    def fake_encode(self, _ffmpeg: str, source: Path, destination: Path) -> None:
        destination.write_bytes(b"OggS-opus-" + source.read_bytes())

    def fake_probe(self, _ffprobe: str, _path: Path) -> dict:
        return {"duration_seconds": 1.0065, "sample_rate_hz": 48000, "channels": 2}

    @patch.object(conversion, "tool_version", return_value="trusted tool 1.0")
    @patch.object(conversion, "canonicalize_manifest")
    @patch.object(conversion, "probe")
    @patch.object(conversion, "encode")
    def test_converts_to_separate_closed_world_candidate_and_preserves_ids(self, encode, probe, _canonicalize, _version):
        encode.side_effect = self.fake_encode
        probe.side_effect = self.fake_probe
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            original = (source / "manifest.json").read_bytes()
            report = conversion.convert(source, root / "opus", max_total_bytes=100_000, pack_version="0.22.0-opus.1", app_version_requirement=">=0.22.0, <0.23.0")
            result = root / "opus"
            manifest = json.loads((result / "manifest.json").read_text())
            asset = manifest["items"][0]["variants"][0]["asset"]
            self.assertEqual(manifest["format_version"], 2)
            self.assertEqual(manifest["pack"]["version"], "0.22.0-opus.1")
            self.assertEqual(manifest["pack"]["app_version_requirement"], ">=0.22.0, <0.23.0")
            self.assertEqual(manifest["items"][0]["id"], "keep-this-id")
            self.assertEqual(asset["path"], "assets/keep-this-id.opus")
            self.assertEqual(asset["codec"], "ogg_opus")
            self.assertIsNone(asset["bit_depth"])
            self.assertEqual(manifest["items"][0]["analysis"]["duration_seconds"], 1.0)
            self.assertEqual(report["items"][0]["validation"]["status"], "technical_metadata_validated")
            self.assertEqual((source / "manifest.json").read_bytes(), original)
            self.assertEqual(sorted(path.relative_to(result).as_posix() for path in result.rglob("*") if path.is_file()), ["assets/keep-this-id.opus", "manifest.json"])
            self.assertEqual(report["items"][0]["id"], "keep-this-id")
            self.assertTrue((root / "opus.conversion-report.json").is_file())

    def test_rejects_existing_or_nested_destination_without_touching_source(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            original = (source / "manifest.json").read_bytes()
            with self.assertRaisesRegex(conversion.ConversionError, "destination"):
                conversion.convert(source, source / "nested")
            existing = root / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(conversion.ConversionError, "destination"):
                conversion.convert(source, existing)
            self.assertEqual((source / "manifest.json").read_bytes(), original)

    def test_dry_run_validates_source_and_has_deterministic_output_mapping(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            plan = conversion.dry_run(source, root / "opus", pack_version="0.22.0-opus.1", app_version_requirement=">=0.22.0, <0.23.0")
            self.assertEqual(plan["format_version"], 2)
            self.assertEqual(plan["outputs"], ["assets/keep-this-id.opus"])
            self.assertFalse((root / "opus").exists())

    @patch.object(conversion, "tool_version", return_value="trusted tool 1.0")
    @patch.object(conversion, "canonicalize_manifest")
    @patch.object(conversion, "probe")
    @patch.object(conversion, "encode")
    def test_creates_a_missing_destination_parent(self, encode, probe, _canonicalize, _version):
        encode.side_effect = self.fake_encode
        probe.side_effect = self.fake_probe
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            result = root / "new-parent" / "opus"
            conversion.convert(
                source,
                result,
                pack_version="0.22.0-opus.1",
                app_version_requirement=">=0.22.0, <0.23.0",
            )
            self.assertTrue((result / "manifest.json").is_file())

    @patch.object(conversion, "tool_version", return_value="trusted tool 1.0")
    @patch.object(conversion, "canonicalize_manifest")
    @patch.object(conversion, "probe")
    @patch.object(conversion, "encode")
    def test_only_out_of_range_safe_loop_is_clamped_and_recorded(self, encode, probe, _canonicalize, _version):
        encode.side_effect = self.fake_encode
        probe.return_value = {"duration_seconds": 0.98, "sample_rate_hz": 48000, "channels": 2}
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            report = conversion.convert(source, root / "opus", pack_version="0.22.0-opus.1", app_version_requirement=">=0.22.0, <0.23.0")
            manifest = json.loads((root / "opus/manifest.json").read_text())
            self.assertEqual(manifest["items"][0]["variants"][0]["safe_regions"][0]["end_seconds"], 0.98)
            self.assertEqual(report["items"][0]["validation"]["safe_region_clamps"], [{"start_seconds": 0.0, "old_end_seconds": 1.0, "new_end_seconds": 0.98}])

    def test_requires_explicit_new_pack_revision_and_compatibility_range(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            with self.assertRaisesRegex(conversion.ConversionError, "pack-version"):
                conversion.dry_run(source, root / "opus")

    def test_rejects_non_closed_world_or_bad_master_hash(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            (source / "extra.txt").write_text("unexpected")
            with self.assertRaisesRegex(conversion.ConversionError, "closed-world"):
                conversion.convert(source, root / "opus")
            (source / "extra.txt").unlink()
            manifest = json.loads((source / "manifest.json").read_text())
            manifest["items"][0]["variants"][0]["asset"]["sha256"] = "0" * 64
            (source / "manifest.json").write_text(json.dumps(manifest))
            with self.assertRaisesRegex(conversion.ConversionError, "hash"):
                conversion.convert(source, root / "opus")

    @patch.object(conversion, "tool_version", return_value="trusted tool 1.0")
    @patch.object(conversion, "canonicalize_manifest")
    @patch.object(conversion, "probe")
    @patch.object(conversion, "encode")
    def test_budget_failure_removes_staging_and_destination(self, encode, probe, _canonicalize, _version):
        encode.side_effect = self.fake_encode
        probe.side_effect = self.fake_probe
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = self.make_source(root)
            with self.assertRaisesRegex(conversion.ConversionError, "over budget"):
                conversion.convert(source, root / "opus", max_total_bytes=1, pack_version="0.22.0-opus.1", app_version_requirement=">=0.22.0, <0.23.0")
            self.assertFalse((root / "opus").exists())
            self.assertFalse(list(root.glob(".opus.opus-staging-*")))


if __name__ == "__main__":
    unittest.main()
