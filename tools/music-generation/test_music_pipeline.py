from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import music_pipeline as pipeline  # noqa: E402


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def analysis() -> dict:
    return {
        "decode": {"duration_seconds": 90.0},
        "measurements": {
            "integrated_lufs": {"value": -20.0},
            "true_peak_dbtp": {"value": -3.0},
            "loudness_range_lu": {"value": 4.0},
            "spectral_centroid_hz": {"value": 1000.0},
            "high_frequency_energy_ratio": {"value": 0.1},
            "onset_density_per_second": {"value": 1.0},
            "clipped_samples": 0,
            "discontinuity_candidates": {"candidate_count": 0},
        },
    }


def plan() -> dict:
    candidate = {
        "id": "steady-candidate-001",
        "seed": 1,
        "activity": "deep_work",
        "genre_ids": ["ambient"],
        "mood_ids": ["steady"],
        "duration_seconds": 90.0,
        "bpm": 80,
        "contains_lyrics": False,
        "contains_speech": False,
        "prompts": {"positive": "instrumental ambient", "negative": "voice"},
        "inference": {"codec": "flac"},
    }
    batch = {
        "id": "test-batch",
        "generator_pin": {"source_commit": "a" * 40},
        "terms_evidence": {
            "output_licence": "test-output-terms",
            "licence_url": "https://example.invalid/licence",
        },
    }
    return {
        "batch": batch,
        "taxonomy": {
            "genres": [{"id": "ambient", "label": "Ambient"}],
            "moods": [{"id": "steady", "label": "Steady"}],
        },
        "candidates": [candidate],
    }


class InternalMusicPipelineTests(unittest.TestCase):
    def make_run(self, root: Path) -> tuple[SimpleNamespace, Path]:
        selected = plan()
        context = SimpleNamespace(plan=selected, path=root / "plan.json", sha256="b" * 64)
        run = root / ".local/music-generation/runs/test-batch"
        candidate = selected["candidates"][0]
        asset, report, evidence, record, log, _ = pipeline.production.candidate_paths(
            run, candidate["id"]
        )
        for path in (asset, report, evidence, record, log):
            path.parent.mkdir(parents=True, exist_ok=True)
        asset.write_bytes(b"fLaC-master")
        report.write_text(json.dumps(analysis()), encoding="utf-8")
        evidence.write_text('{"evidence":true}', encoding="utf-8")
        record.write_text(
            json.dumps(
                {
                    "schema": "adhd-music.candidate-ledger.generated",
                    "schema_version": 1,
                    "lifecycle": "generated",
                    "candidate": candidate,
                    "batch": selected["batch"],
                    "verified": {
                        "file_name": asset.name,
                        "bytes": asset.stat().st_size,
                        "codec": "flac",
                        "sample_rate_hz": 48_000,
                        "channels": 2,
                        "sha256": digest(asset),
                        "analyzer_file_name": report.name,
                        "analyzer_sha256": digest(report),
                    },
                    "evidence": {"file_name": evidence.name, "sha256": digest(evidence)},
                }
            ),
            encoding="utf-8",
        )
        log.write_text("complete", encoding="utf-8")
        return context, run

    def test_builds_separate_closed_world_flac_pack_from_exact_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            context, _ = self.make_run(root)
            output = root / "output/flac-pack"
            with (
                mock.patch.object(pipeline.production, "validate", return_value=context),
                mock.patch.object(pipeline.production, "verify_run_identity"),
                mock.patch.object(pipeline.opus, "canonicalize_manifest"),
            ):
                result = pipeline.build_flac_pack(
                    root,
                    root / "plan.json",
                    "test-batch",
                    output,
                    pack_id="internal.test",
                    pack_title="Internal Test",
                    pack_version="1.0.0-flac.1",
                    app_version_requirement=">=0.2.1, <0.3.0",
                )

            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(result["items"], 1)
            self.assertEqual(manifest["items"][0]["id"], "internal.test.steady-candidate-001")
            self.assertEqual(manifest["items"][0]["human_qa"], {"status": "draft", "reviews": []})
            self.assertEqual(
                {path.relative_to(output).as_posix() for path in output.rglob("*") if path.is_file()},
                {"manifest.json", "assets/steady-candidate-001.flac"},
            )

    def test_tampered_generated_record_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            context, run = self.make_run(root)
            candidate = context.plan["candidates"][0]
            asset, report, evidence, record, _, _ = pipeline.production.candidate_paths(
                run, candidate["id"]
            )
            value = json.loads(record.read_text(encoding="utf-8"))
            value["verified"]["sha256"] = "0" * 64
            record.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(pipeline.PipelineError, "differs"):
                pipeline.validate_generated_candidate(
                    candidate, context.plan["batch"], asset, report, evidence, record
                )

    def test_package_keeps_flac_and_opus_outputs_distinct_and_records_sizes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            flac = root / "flac"
            opus = root / "opus"
            flac.mkdir()
            (flac / "manifest.json").write_text("{}", encoding="utf-8")
            converted = {
                "items": [{"id": "one"}],
                "total_bytes": 123,
                "conversion": {"bitrate_kbps": 112},
            }
            with (
                mock.patch.object(
                    pipeline,
                    "build_flac_pack",
                    return_value={
                        "path": str(flac),
                        "items": 1,
                        "bytes": 456,
                        "manifest_sha256": "a" * 64,
                    },
                ),
                mock.patch.object(pipeline.opus, "convert", return_value=converted) as convert,
            ):
                result = pipeline.package_run(
                    root,
                    root / "plan.json",
                    "test-batch",
                    root / "flac-output",
                    opus,
                    pack_id="internal.test",
                    pack_title="Internal Test",
                    flac_version="1.0.0-flac.1",
                    opus_version="1.0.0-opus.1",
                    app_version_requirement=">=0.2.1, <0.3.0",
                    ffmpeg="ffmpeg",
                    ffprobe="ffprobe",
                    max_total_bytes=300_000_000,
                )
            self.assertEqual(result["opus"]["bytes"], 123)
            self.assertEqual(result["opus"]["bitrate_kbps"], 112)
            self.assertTrue(Path(result["report"]).is_file())
            self.assertEqual(convert.call_args.kwargs["pack_version"], "1.0.0-opus.1")

    def test_rejects_nested_outputs_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(pipeline.PipelineError, "non-nested"):
                pipeline.package_run(
                    root,
                    root / "plan.json",
                    "test-batch",
                    root / "candidate",
                    root / "candidate/opus",
                    pack_id="internal.test",
                    pack_title="Internal Test",
                    flac_version="1.0.0-flac.1",
                    opus_version="1.0.0-opus.1",
                    app_version_requirement=">=0.2.1, <0.3.0",
                    ffmpeg="ffmpeg",
                    ffprobe="ffprobe",
                    max_total_bytes=300_000_000,
                )

    def test_rejects_existing_pipeline_report_before_building(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            opus = root / "opus"
            report = root / "opus.pipeline-report.json"
            report.write_text("do not replace", encoding="utf-8")
            with (
                mock.patch.object(pipeline, "build_flac_pack") as build,
                self.assertRaisesRegex(pipeline.PipelineError, "overwrite"),
            ):
                pipeline.package_run(
                    root,
                    root / "plan.json",
                    "test-batch",
                    root / "flac",
                    opus,
                    pack_id="internal.test",
                    pack_title="Internal Test",
                    flac_version="1.0.0-flac.1",
                    opus_version="1.0.0-opus.1",
                    app_version_requirement=">=0.2.1, <0.3.0",
                    ffmpeg="ffmpeg",
                    ffprobe="ffprobe",
                    max_total_bytes=300_000_000,
                )
            build.assert_not_called()
            self.assertEqual(report.read_text(encoding="utf-8"), "do not replace")


if __name__ == "__main__":
    unittest.main()
