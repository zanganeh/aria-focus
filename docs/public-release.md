# Aria Focus public release

The 0.3.0 line is the first stable release track. A tag must be a canonical
stable `vMAJOR.MINOR.PATCH` tag; prerelease suffixes are rejected by
`scripts/verify_release_tag.py`. The first reviewed and signed public candidate
is blocked until every gate in this document passes.

Windows installer metadata uses the numeric version `0.3.0` because MSI does not
accept text prerelease identifiers; the app, packages, About panel, and Git tag all
carry `0.3.0`.

## External approvals

- Complete a basic Australian and US name/trademark search for "Aria Focus".
- Approve redistribution terms for every bundled generated track.
- Approve redistribution of the optional Music Studio model/runtime payload.
- Apply to SignPath Foundation and configure origin-verified GitHub signing.

These approvals are not yet complete. The workflow must fail closed until they are.

## Content staging

Production audio and runtime files remain outside Git. Stage the reviewed
100-track archive in the `aria-focus-library-v1` GitHub release, pin its exact
asset name and SHA-256 in `release/public-release-assets.json`, and update its
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

The public signed release path covers Windows MSI and NSIS installers. The
release workflow also builds and uploads source-only `.app` and `.dmg` artifacts
for Apple Silicon (`aarch64`) and Intel (`x86_64`) to the draft release, without
the reviewed private-beta music or Windows-only Music Studio runtime. Publishing
those macOS files as customer downloads requires Apple Developer ID signing and
notarization credentials, which are not assumed here.

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

The repository workflow performs the tag creation, build, SignPath submission,
Authenticode verification, checksums, and draft-release upload. Dispatch it from
the protected `public-release` environment with a new stable version and the
source ref to release; it creates the tag only after the content and code gates
pass. It needs the documented SignPath secret/variables configured in GitHub.
Follow
[`releases.md`](releases.md) for setup, dispatch, and final publication.

The NSIS installer is the primary customer download. All public builds must use a
new stable version and follow the signed workflow.

## Updater signing

The app includes the official Tauri 2 updater plugin, but update installation is
not live until the matching public key and signed metadata exist. Configure
`TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as GitHub Actions secrets and
`TAURI_UPDATER_PUBLIC_KEY` as a repository variable. The release workflow then
creates updater archives, signatures, and `latest.json` only when all three are
present; otherwise it intentionally publishes no updater metadata. Never commit
the private key.
