# GitHub release process

Aria Focus uses two separate GitHub Actions paths:

- `.github/workflows/ci.yml` builds and tests source changes and produces
  unsigned source-only Windows and macOS artifacts for inspection. Windows
  remains MSI/NSIS; macOS is app/DMG for Apple Silicon (`aarch64`) and Intel
  (`x86_64`), with separate CI artifacts for each architecture.
- `.github/workflows/public-release.yml` is a tag-triggered, protected release workflow
  for reviewed content and signed public Windows/macOS installers. Manual dispatch
  remains available as a recovery path and can optionally publish the verified draft.

CI artifacts are never official releases.

## Stable release contract

Aria Focus releases are stable `vMAJOR.MINOR.PATCH` tags. The release validator
(`scripts/verify_release_tag.py`) accepts only canonical stable tags and rejects
prerelease suffixes such as `-beta.1`, `-alpha.2`, or `-rc.3`. The 0.3.0 line is the
first stable release track; do not reuse earlier `0.2.x` beta tags for stable builds.

Windows installer metadata uses the numeric version because MSI does not accept
text prerelease identifiers. Because releases are now stable, the app, packages,
About panel, Git tag, and Tauri installer version all carry the same `0.3.0`.

Code signing, notarization, the reviewed-library archive, and the Music Studio
runtime are protected release gates. The workflow fails closed until those gates
are satisfied; it does not claim signing or approval has already occurred.

The updater is also fail-closed until its metadata is configured. The checked-in
Tauri configuration contains the literal placeholder
`REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY`; an unavailable endpoint or invalid key is
silently ignored by the app. The public release workflow creates updater archives
and `latest.json` only when all of these are configured:

- GitHub secret `TAURI_SIGNING_PRIVATE_KEY`;
- GitHub secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; and
- repository variable `TAURI_UPDATER_PUBLIC_KEY` containing the matching public
  key (never the private key).

Generate a key pair locally with `pnpm tauri signer generate -w
~/.tauri/aria-focus.key`, store the private key and password only in the two
GitHub secrets, and put the generated public key in the repository variable and
in a reviewed config update. The guarded workflow build creates the signed NSIS
updater archive, its `.sig`, and a Windows `latest.json` entry pointing at the
same stable GitHub release. It does not produce updater metadata when the
secrets/variable are absent.

The updater uses Tauri's signed `downloadAndInstall()` flow and the process
plugin's relaunch only after the user clicks “Download and restart”; it never
downloads arbitrary release files or restarts on its own.

Pushing a stable version tag automatically starts signed draft creation. The
workflow still requires protected-environment approval. Manual dispatch can
publish the verified draft with `publish_release`; otherwise it remains a draft
for installed-app testing. Ordinary branch pushes never create releases.

## One-time repository setup

1. Create the public GitHub repository `zanganeh/aria-focus` and make `main` the
   default branch.
2. Enable private vulnerability reporting and branch protection for `main`.
3. Create a `public-release` environment with required maintainer approval and
   restrict it to protected version tags.
4. Configure the SignPath GitHub integration and add:
   - secret `SIGNPATH_API_TOKEN`;
   - variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`,
     `SIGNPATH_SIGNING_POLICY_SLUG`, and
     `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`.
5. Export a Developer ID Application `.p12` certificate as base64 and add the
   Apple Developer environment configuration:
   - secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`,
     `APPLE_PASSWORD` (an app-specific password), and `KEYCHAIN_PASSWORD`;
   - variables `APPLE_TEAM_ID` and `APPLE_SIGNING_IDENTITY` (the workflow also
     verifies the imported Developer ID identity before building).
6. Keep the workflow's `GITHUB_TOKEN` permission at the declared minimum. The
   release job needs `contents: write` only to create the draft release and upload
   its signed files.

The environment gate is mandatory. A workflow file in a pull request must not be
able to access signing credentials or publish a release without approval.

## Prepare the reviewed library

The audio library is not stored in Git.

1. Complete provenance, technical analysis, two-person listening review, and
   redistribution approval for all 100 tracks.
2. Build a closed-world ZIP containing only `manifest.json` and its declared
   assets.
3. Create the separate GitHub release named by
   `release/public-release-assets.json` and upload the ZIP under the exact
   configured filename.
4. Compute its lowercase SHA-256 and replace
   `REPLACE_AFTER_LIBRARY_APPROVAL` in that configuration.
5. Run `python scripts/verify_release_tag.py v<version>` and the complete local
   verification suite.

Changing the library tag, filename, or hash requires review like a source-code
change. The release workflow never downloads an unpinned "latest" asset.

## Prepare a version

Use one version consistently in root `package.json`, the desktop `package.json`,
and `[workspace.package]` in `Cargo.toml`. Tauri's Windows bundle version is the
numeric core; for a stable release it equals the package version. For example:

- source version: `0.3.0`;
- Git tag: `v0.3.0`;
- Tauri installer version: `0.3.0`.

Update release notes and run:

```powershell
pnpm verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python scripts/check_repository_hygiene.py
python scripts/verify_release_tag.py v0.3.0
```

## Automatic protected workflow

For a manual stable release, open **Actions → Signed public release → Run
workflow**, select the reviewed `source_ref`, enter the matching stable tag, and
choose whether `publish_release` should publish the verified draft. The workflow
validates the library and signing configuration, builds and signs the packages,
and creates the tag only after all gates pass. Watch it with:

```powershell
gh run watch
```

Pushing an already-reviewed stable version tag also starts `public-release.yml`:

```powershell
git push origin v0.3.0
```

The workflow fails closed if the trigger tag, project
versions, reviewed-library pin, content manifest, tests, or signatures differ.

After approval, it:

1. downloads and stages the pinned reviewed library;
2. runs repository, content, frontend, and Rust verification;
3. builds NSIS and MSI packages;
4. uploads the unsigned pair as workflow evidence;
5. submits that GitHub-origin artifact to SignPath;
6. verifies both returned Authenticode signatures;
7. writes `SHA256SUMS`; and
8. creates or updates a **draft release** (not a prerelease) for the existing tag.

The macOS jobs run on Apple Silicon and Intel runners, import the protected
Developer ID certificate, build the reviewed library, sign the app, notarize the
DMG through Tauri's configured Apple credentials, staple the ticket, and validate
it before uploading.

## Publish the draft

Do not publish immediately after automation finishes. Download the signed draft
assets and complete the Windows matrix in `docs/content-pack-upgrades.md`,
including a clean profile and an upgrade profile containing retired v1 plus
current v2. Confirm offline activity-tile playback on a real audio device.

Compare downloaded file hashes with `SHA256SUMS`, record the manual results in
the release notes, then publish the draft through GitHub. If any gate fails,
leave the draft unpublished and cut a new version; never replace a published
installer silently.
