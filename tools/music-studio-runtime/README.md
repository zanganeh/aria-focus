# Music Studio runtime package

Run `build_runtime_package.py` explicitly from release automation with `--source`, a new `--output`, `--version`, and an Ed25519 PEM private key via `--private-key` or `MUSIC_STUDIO_RUNTIME_SIGNING_KEY`. The matching public key is pinned in the desktop source asset. `--dry-run` validates inputs and prints the deterministic manifest without copying runtime files.

After the package is fully verified, `build_runtime_distribution.py` converts it
to a deterministic split tar stream. Every part stays below the GitHub Release
per-file limit, and a second signed manifest pins the package-manifest hash,
unpacked size, part order, size, and SHA-256. Upload the two distribution
documents and every `.part` file to the exact release URL pinned by the app.
