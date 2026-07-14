# Aria Focus public beta release

`v0.2.1-beta.1` is an explicitly unsigned listening-test preview and does not
satisfy this release gate. The first reviewed and signed public candidate must use
a newer version and is blocked until every gate in this document passes.

Windows installer metadata uses numeric version `0.2.1` because MSI does not
accept text prerelease identifiers; the app, packages, About panel, and Git tag
retain the public-beta label.

## External approvals

- Complete a basic Australian and US name/trademark search for “Aria Focus”.
- Approve redistribution terms for every bundled generated track.
- Approve redistribution of the optional Music Studio model/runtime payload.
- Apply to SignPath Foundation and configure origin-verified GitHub signing.

## Content staging

Production audio and runtime files remain outside Git. Stage the reviewed
100-track archive in the `aria-focus-library-v1` GitHub release, pin its exact
asset name and SHA-256 in `release/public-beta-assets.json`, and update its
customer-facing manifest title to `Aria Focus Library`. The signed-release
workflow downloads and safely stages that immutable asset before running:

```powershell
python scripts/verify_public_library.py
```

The gate requires 100 closed-world assets, exactly 20 per activity, two-reviewer
approval, licence evidence, instrumental provenance, valid hashes, and a total
size that keeps the installer below GitHub's 2 GiB per-file limit.

Build the optional Studio distribution with the same Ed25519 release key used
for the verified package, then upload all signed documents and `.part` files to
the pinned `studio-runtime-v1.0.0` release.

## Signed installers

Build the release configuration with the reviewed library:

```powershell
pnpm -C apps/desktop tauri build --features bundled-library --config src-tauri/tauri.release.conf.json
```

Submit the NSIS and MSI outputs to the approved SignPath project. Verify every
returned Authenticode signature, produce `SHA256SUMS`, generate an SBOM and
dependency licence report, perform clean-install and legacy-data-migration E2E,
then attach only the signed artifacts to the public GitHub release.

The legacy-data E2E must follow the complete matrix in
[`content-pack-upgrades.md`](content-pack-upgrades.md), including the retired-v1/current-v2 case.
Do not publish a build whose content tests ran only against an empty profile.

The repository workflow performs the build, SignPath submission, Authenticode
verification, checksums, and draft-release upload. It must be manually dispatched
from the exact version tag through the protected `public-release` environment and
needs the documented SignPath secret/variables configured in GitHub. Follow
[`releases.md`](releases.md) for setup, dispatch, and final publication.

The NSIS installer is the primary customer download. Do not publish another
unsigned installer or replace the immutable `v0.2.1-beta.1` preview assets. All
later public builds must use a new version and follow the signed workflow.
