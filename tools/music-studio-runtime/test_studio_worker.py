import contextlib
import importlib.util
import io
import json
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("studio_worker.py")
SPEC = importlib.util.spec_from_file_location("studio_worker", MODULE_PATH)
worker = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(worker)


class WorkerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "ace-step-source").mkdir()
        (self.root / "ace-step-source" / "cli.py").write_text("", encoding="utf-8")
        (self.root / "snapshots" / "turbo-vae").mkdir(parents=True)
        (self.root / "snapshots" / "planner-0.6b").mkdir()
        self.request = self.root / "request.json"
        self.request.write_text(json.dumps({
            "positive_prompt": "steady ambient focus",
            "negative_prompt": "vocals, speech",
            "duration_seconds": 90,
            "seed": 42,
        }), encoding="utf-8")
        self.output = self.root / "output"

    def invoke(self, side_effect):
        captured = {}
        def run(argv, **kwargs):
            captured["argv"] = argv
            captured["kwargs"] = kwargs
            config_path = Path(argv[-1])
            captured["config"] = tomllib.loads(config_path.read_text(encoding="utf-8"))
            return side_effect(self.output)
        stdout = io.StringIO()
        with mock.patch.object(worker, "__file__", str(self.root / "studio_worker.py")), \
             mock.patch.object(worker.subprocess, "run", side_effect=run), \
             contextlib.redirect_stdout(stdout):
            code = worker.main(["--request", str(self.request), "--output-dir", str(self.output)])
        return code, json.loads(stdout.getvalue()), captured

    def test_exact_flat_toml_and_argv(self):
        def success(output):
            (output / "generated.flac").write_bytes(b"fLaC")
            return subprocess.CompletedProcess([], 0)
        code, result, captured = self.invoke(success)
        self.assertEqual(code, 0)
        self.assertEqual(result, {"ok": True, "output": "draft.flac"})
        self.assertEqual(captured["argv"][1:5], [
            str(self.root / "ace-step-source" / "cli.py"), "--backend", "pt", "--config"
        ])
        self.assertEqual(len(captured["argv"]), 6)
        environment = captured["kwargs"]["env"]
        self.assertEqual(environment["ACESTEP_CHECKPOINTS_DIR"], str(self.root / "snapshots" / "turbo-vae"))
        self.assertEqual(environment["HF_HUB_OFFLINE"], "1")
        self.assertEqual(environment["TRANSFORMERS_OFFLINE"], "1")
        cache = Path(environment["HF_HOME"])
        self.assertTrue(cache.name.startswith("adhd-music-studio-cache-"))
        self.assertEqual(environment["HF_MODULES_CACHE"], str(cache / "modules"))
        config = captured["config"]
        self.assertNotIn("generation", config)
        expected = {
            "project_root": str(self.root / "ace-step-source"),
            "checkpoint_dir": str(self.root / "snapshots" / "turbo-vae"),
            "lm_model_path": str(self.root / "snapshots" / "planner-0.6b"),
            "backend": "pt", "device": "cuda", "save_dir": str(self.output),
            "audio_format": "flac", "caption": "steady ambient focus",
            "lyrics": "[Instrumental]", "instrumental": True, "duration": 90,
            "inference_steps": 8, "seed": 42, "shift": 3, "infer_method": "ode",
            "use_random_seed": False, "lm_negative_prompt": "vocals, speech",
            "batch_size": 1, "cfg_interval_end": 1.0, "cfg_interval_start": 0.0,
            "guidance_scale": 7.0, "lm_cfg_scale": 2.0, "lm_temperature": 0.8,
            "lm_top_k": 0, "lm_top_p": 0.9, "thinking": False,
            "timesignature": "4/4", "use_adg": False,
        }
        self.assertEqual(config, expected)
        self.assertTrue((self.output / "draft.flac").is_file())

    def test_invalid_request_does_not_spawn(self):
        self.request.write_text('{"duration_seconds":12}', encoding="utf-8")
        with mock.patch.object(worker, "__file__", str(self.root / "studio_worker.py")), \
             mock.patch.object(worker.subprocess, "run") as run:
            self.assertEqual(worker.main(["--request", str(self.request), "--output-dir", str(self.output)]), 1)
            run.assert_not_called()

    def test_missing_and_multiple_outputs_are_rejected(self):
        for count in (0, 2):
            with self.subTest(count=count):
                output = self.root / f"output-{count}"
                self.output = output
                def result(path, count=count):
                    for index in range(count):
                        (path / f"{index}.flac").write_bytes(b"fLaC")
                    return subprocess.CompletedProcess([], 0)
                code, value, _ = self.invoke(result)
                self.assertEqual((code, value["code"]), (1, "invalid_output"))

    def test_timeout_is_bounded_result(self):
        def timeout(_):
            raise subprocess.TimeoutExpired([], 1)
        code, value, _ = self.invoke(timeout)
        self.assertEqual((code, value), (1, {"ok": False, "code": "timeout"}))


if __name__ == "__main__":
    unittest.main()
