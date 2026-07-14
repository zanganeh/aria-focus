import importlib.util, tempfile, unittest
from pathlib import Path

spec=importlib.util.spec_from_file_location("builder",Path(__file__).with_name("build_runtime_package.py")); builder=importlib.util.module_from_spec(spec); spec.loader.exec_module(builder)
try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption
except ImportError: Ed25519PrivateKey=None

@unittest.skipIf(Ed25519PrivateKey is None,"cryptography is not installed; signing tests skipped")
class PackageTests(unittest.TestCase):
 def test_manifest_is_sorted_and_runtime_matches(self):
  with tempfile.TemporaryDirectory() as d:
   root=Path(d); source=root/'source'; source.mkdir()
   for name in builder.SOURCE_INCLUDE:
    p=source/name
    if name.endswith('.json'): p.write_text('{}')
    else: p.mkdir(); (p/'z').write_text(name)
   key=root/'key.pem'; key.write_bytes(Ed25519PrivateKey.generate().private_bytes(Encoding.PEM,PrivateFormat.PKCS8,NoEncryption()))
   output=root/'out'; self.assertEqual(builder.main.__name__,'main')
   import subprocess,sys,json
   subprocess.run([sys.executable,str(Path(builder.__file__)),'--source',str(source),'--output',str(output),'--version','v1','--private-key',str(key)],check=True)
   data=json.loads((output/'package-manifest.json').read_text()); self.assertEqual(data['files'],sorted(data['files'],key=lambda x:x['path']))
   self.assertTrue(all((output/'runtime'/f['path']).is_file() for f in data['files']))
   self.assertIn('studio_worker.py', {f['path'] for f in data['files']})
   self.assertTrue((output/'runtime'/'studio_worker.py').is_file())
