# GitHub release process

Aria Focus uses two separate GitHub Actions paths:

- `.github/workflows/ci.yml` builds and tests source changes and produces
  unsigned source-only Windows artifacts for inspection.
- `.github/workflows/public-beta.yml` is a manually dispatched, protected release
  workflow for reviewed content and signed public installers.

CI artifacts are never official releases.

## Current unsigned preview

`v0.2.1-beta.1` was explicitly published as an unsigned prerelease so early users
can test the complete offline app before final music review and code signing. It
is not produced by the signed workflow and must not be promoted to a signed or
reviewed build in place. Any corrected, reviewed, or signed installer must use a
new version and immutable release assets.

Release publication is not automatic. Pushes and pull requests run CI, while a
public release always requires an explicit tag and maintainer action. The signed
workflow additionally requires protected-environment approval and creates a draft
that must be published manually after installed-app testing.

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
5. Keep the workflow's `GITHUB_TOKEN` permission at the declared minimum. The
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
   `release/public-beta-assets.json` and upload the ZIP under the exact configured
   filename.
4. Compute its lowercase SHA-256 and replace
   `REPLACE_AFTER_LIBRARY_APPROVAL` in that configuration.
5. Run `python scripts/verify_release_tag.py v<version>` and the complete local
   verification suite.

Changing the library tag, filename, or hash requires review like a source-code
change. The release workflow never downloads an unpinned “latest” asset.

## Prepare a version

Use one version consistently in root `package.json`, the desktop `package.json`,
and `[workspace.package]` in `Cargo.toml`. Tauri's Windows bundle version is the
numeric core because installers do not accept a prerelease suffix. For example:

- source version: `0.2.1-beta.1`;
- Git tag: `v0.2.1-beta.1`;
- Tauri installer version: `0.2.1`.

Update release notes and run:

```powershell
pnpm verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python scripts/check_repository_hygiene.py
python scripts/verify_release_tag.py v0.2.1-beta.1
```

Create an annotated or signed tag only from the reviewed commit:

```powershell
git tag -s v0.2.1-beta.1 -m "Aria Focus 0.2.1 beta 1"
git push origin v0.2.1-beta.1
```

## Run the protected workflow

Dispatch the workflow from the same tag and pass the tag as its input:

```powershell
gh workflow run public-beta.yml `
  --ref v0.2.1-beta.1 `
  -f release_tag=v0.2.1-beta.1
gh run watch
```

The workflow fails closed if the dispatch ref, input tag, project versions,
reviewed-library pin, content manifest, tests, or signatures differ.

After approval, it:

1. downloads and stages the pinned reviewed library;
2. runs repository, content, frontend, and Rust verification;
3. builds NSIS and MSI packages;
4. uploads the unsigned pair as workflow evidence;
5. submits that GitHub-origin artifact to SignPath;
6. verifies both returned Authenticode signatures;
7. writes `SHA256SUMS`; and
8. creates or updates a **draft prerelease** for the existing tag.

## Publish the draft

Do not publish immediately after automation finishes. Download the signed draft
assets and complete the Windows matrix in `docs/content-pack-upgrades.md`,
including a clean profile and an upgrade profile containing retired v1 plus
current v2. Confirm offline activity-tile playback on a real audio device.

Compare downloaded file hashes with `SHA256SUMS`, record the manual results in
the release notes, then publish the draft through GitHub. If any gate fails,
leave the draft unpublished and cut a new version; never replace a published
installer silently.
