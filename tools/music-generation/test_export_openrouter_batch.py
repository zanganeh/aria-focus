import unittest

from export_openrouter_batch import build_analysis, canonical_analysis


class ExportAnalysisTests(unittest.TestCase):
    def test_release_gain_resolves_provider_full_scale_clipping(self):
        report = {
            "decode": {"duration_seconds": 180.0},
            "measurements": {
                "integrated_lufs": {"value": -11.0},
                "true_peak_dbtp": {"value": 0.25},
                "loudness_range_lu": {"value": 7.0},
                "spectral_centroid_hz": {"value": 600.0},
                "high_frequency_energy_ratio": {"value": 0.01},
                "onset_density_per_second": {"value": 1.5},
                "clipped_samples": 45,
                "discontinuity_candidates": {"candidate_count": 0},
                "silence": {"total_near_silence_seconds": 0.1},
            },
            "hard_rejections": [
                {"code": "clipped_samples", "message": "full-scale samples"}
            ],
        }
        analysis = build_analysis(
            report,
            {
                "duration_seconds": 180.0,
                "sample_rate_hz": 48_000,
                "channels": 2,
                "bit_depth": 16,
            },
            {"bpm": 104},
        )
        self.assertEqual(analysis["hard_rejections"], [])
        self.assertEqual(analysis["clipped_samples"], 0)
        self.assertEqual(analysis["source_processing"]["source_clipped_samples"], 45)
        self.assertAlmostEqual(analysis["integrated_lufs"], -14.0)
        self.assertAlmostEqual(analysis["true_peak_dbfs"], -2.75)

    def test_release_gain_keeps_unrelated_hard_rejections(self):
        report = {
            "decode": {"duration_seconds": 180.0},
            "measurements": {"clipped_samples": 0},
            "hard_rejections": [
                {"code": "discontinuity", "message": "candidate"}
            ],
        }
        analysis = build_analysis(
            report,
            {
                "duration_seconds": 180.0,
                "sample_rate_hz": 48_000,
                "channels": 2,
                "bit_depth": 16,
            },
            {"bpm": 104},
        )
        self.assertEqual(analysis["hard_rejections"][0]["code"], "discontinuity")

    def test_canonical_analysis_rejects_unresolved_findings_and_strips_internal_fields(self):
        report = {
            "decode": {"duration_seconds": 180.0},
            "measurements": {"clipped_samples": 0},
            "hard_rejections": [],
        }
        analysis = build_analysis(
            report,
            {"duration_seconds": 180.0, "sample_rate_hz": 48_000, "channels": 2, "bit_depth": 16},
            {"bpm": 104},
        )
        canonical = canonical_analysis(analysis, "item-1")
        self.assertNotIn("hard_rejections", canonical)
        self.assertNotIn("source_processing", canonical)
        self.assertIn("duration_seconds", canonical)

        analysis["hard_rejections"] = [{"code": "unexplained_near_silence"}]
        with self.assertRaisesRegex(RuntimeError, "item-1"):
            canonical_analysis(analysis, "item-1")


if __name__ == "__main__":
    unittest.main()
