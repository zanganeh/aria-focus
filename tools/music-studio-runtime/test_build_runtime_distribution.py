import base64
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
except ImportError:
    Ed25519PrivateKey = None

import build_runtime_distribution as distribution


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography is not installed")
class RuntimeDistributionTests(unittest.TestCase):
    def test_builds_signed_deterministic_split_tar(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package_dir = root / "package"
            output = root / "distribution"
            runtime = package_dir / "runtime"
            runtime.mkdir(parents=True)
            payload = b"worker" * 500
            (runtime / "studio_worker.py").write_bytes(payload)
            key = Ed25519PrivateKey.generate()
            digest = distribution.hashlib.sha256(payload).hexdigest()
            package_manifest = {
                "files": [{"bytes": len(payload), "path": "studio_worker.py", "sha256": digest}],
                "format": 1,
                "required_bytes": len(payload),
                "runtime_version": "test-v1",
            }
            package_bytes = json.dumps(
                package_manifest, sort_keys=True, separators=(",", ":")
            ).encode()
            (package_dir / "package-manifest.json").write_bytes(package_bytes)
            (package_dir / "package-manifest.sig").write_text(
                base64.b64encode(key.sign(package_bytes)).decode() + "\n", encoding="ascii"
            )

            manifest = distribution.build(package_dir, output, key, 1024)
            self.assertGreater(len(manifest["chunks"]), 1)
            self.assertTrue(all(chunk["bytes"] <= 1024 for chunk in manifest["chunks"]))
            canonical = (output / "runtime-distribution.json").read_bytes()
            key.public_key().verify(
                base64.b64decode((output / "runtime-distribution.sig").read_text().strip()),
                canonical,
            )
            combined = b"".join(
                (output / chunk["file_name"]).read_bytes() for chunk in manifest["chunks"]
            )
            extracted = root / "extracted"
            extracted.mkdir()
            with tarfile.open(fileobj=io.BytesIO(combined), mode="r:") as archive:
                archive.extractall(extracted, filter="data")
            self.assertEqual(
                (extracted / "runtime/studio_worker.py").read_bytes(),
                payload,
            )
            self.assertEqual(json.loads(canonical), manifest)


if __name__ == "__main__":
    unittest.main()
