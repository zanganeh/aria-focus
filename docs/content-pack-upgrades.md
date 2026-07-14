# Content-pack upgrade and retirement safety

This checklist is a release gate for every app-version, app-identifier, bundled
library, codec, or pack-version change. It exists because startup was broken
repeatedly by legacy installed-pack state that a clean-install test did not
contain.

## Startup invariants

1. Classify a registry row as current, generated, imported, or explicitly
   retired **before** applying current app-version compatibility rules.
2. A retired owner-waived pack must never participate in playback selection and
   must never make the current content service unavailable. Its historical
   `app_version_requirement` is expected to exclude newer applications.
3. Retired rows remain part of the closed-world directory audit until a separate,
   transactional retirement migration removes both registry metadata and files.
   Do not silently delete user data during ordinary startup.
4. Every current pack remains subject to strict manifest, registry-path, hash,
   file-tree, codec, and app-version validation. Retirement is an explicit ID
   allow-list, not a general validation bypass.
5. An application identifier or brand-directory change must transactionally
   rebase known installed-pack paths from the exact legacy root to the exact new
   root. Never accept arbitrary or relative stored paths.
6. A bundled-library replacement may preserve feedback only when the old and new
   item-ID sets are exactly equal.

The implementation order in `PackService::validated_records` is therefore:

1. reconcile durable receipts;
2. load registry rows;
3. exclude explicitly retired rows from current validation and selection;
4. strictly validate every active row;
5. audit directories against **all** registry rows, including retired rows.

Filtering retired packs after step 4 is a regression: the obsolete compatibility
range will fail before the filter runs.

## Required automated evidence

The test
`retired_owner_waived_pack_with_historical_app_range_does_not_block_startup`
must run with the bundled-listening feature. Its fixture deliberately keeps the
retired v1 row at `>=0.1.0, <0.2.0` while the current application is newer.

Run:

```powershell
$env:ARIA_FOCUS_BUNDLED_PACK_DIR = '../../../content/opus-release/private-beta-pack'
cargo test -p aria-focus-desktop --features bundled-listening-test `
  retired_owner_waived_pack_with_historical_app_range_does_not_block_startup --lib
cargo test -p aria-focus-desktop --features bundled-listening-test --lib
cargo clippy -p aria-focus-desktop --all-targets `
  --features bundled-listening-test -- -D warnings
```

A release is blocked if the feature-specific regression test is skipped because
the build has no pinned successor pack.

## Windows upgrade test matrix

Do not approve an installer using only a clean profile. Test these states:

- no prior installation;
- current pack already installed;
- retired v1 plus current v2 installed;
- paths stored under the legacy application identifier;
- owner-waived FLAC v2 upgraded to owner-waived Ogg Opus v2;
- feedback and session history already present.

For each upgrade:

1. Back up `preferences.sqlite3`.
2. Install the previous released/listening-test build and launch it once.
3. Install the candidate over it without deleting AppData.
4. Confirm startup health contains no unavailable content service.
5. Confirm the current v2 row uses the new app-data root and expected manifest
   hash, all 100 Opus assets exist, and the retired v1 row does not appear in the
   playable catalogue.
6. Start at least one activity tile and confirm audio begins.
7. Confirm existing feedback and session history remain unchanged.

Record the installer SHA-256, application version, old and new registry rows,
and test result in the release evidence. A process merely staying open is not
enough evidence because the recovery screen also keeps the process alive.

## Adding another retired pack

Adding an ID to `RETIRED_PRIVATE_BETA_PACK_IDS` requires all of the following in
the same change:

- a reason and successor pack in the release notes;
- a historical-compatibility regression fixture;
- proof that its ID is not the current pinned pack ID;
- proof that it is excluded from selection but retained in the directory audit;
- a decision on whether a later transactional cleanup migration is warranted.

Never broaden retirement matching by prefix, status alone, version comparison,
or manifest-provided metadata.
