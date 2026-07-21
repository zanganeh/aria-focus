# Contributing to Aria Focus

Thank you for helping make offline focus music more accessible.

1. Open an issue before a large product, audio, persistence, or security change.
2. Keep pull requests focused and include tests for changed behaviour.
3. Run `pnpm verify`, `cargo test --workspace`, and
   `cargo clippy --workspace --all-targets -- -D warnings`.
4. Do not commit generated music, model weights, local runtimes, secrets, or
   release binaries.
5. Do not add health or treatment claims. Audio publication requires the
   existing provenance, analysis, and human-review gates.
6. For changes to pack IDs, manifests, install paths, bundled assets, or startup
   validation, follow [`docs/content-pack-upgrades.md`](docs/content-pack-upgrades.md)
   and test an existing-user upgrade profile. A clean profile alone cannot prove
   that a release preserves playback.

Unless explicitly stated otherwise, contributions are submitted under the
project's MIT OR Apache-2.0 terms. Brand assets are not accepted through normal
code contributions without maintainer approval.
