//! Versioned metadata catalogue and hostile-input-safe offline pack staging.
//!
//! This crate is device, Tauri, SQLite, decoder, and audio-callback independent.
//! It validates packs and performs deterministic metadata-only playback
//! selection; native media preparation stays in `audio-engine`.

pub mod import;
pub mod manifest;
pub mod selection;

pub use import::{
    hash_file_sha256, stage_pack, verify_bundled_owner_waived_pack, verify_generated_local_pack,
    verify_installed_pack, ImportLimits, PackImportError, StagedPack,
};
pub use manifest::{
    canonical_manifest_bytes, canonical_pack_path, is_stable_identifier, AssetCodec,
    ContentPackManifest, GeneratedLocalRecord, LocalGenerationEvidence, ManifestValidationError,
    SafeRegionKind, TaxonomyTerm, MANIFEST_PATH,
};
pub use selection::{
    available_genres, available_genres_with_eligibility, available_moods,
    available_moods_with_eligibility, select_playback_plan,
    select_playback_plan_for_item_with_eligibility, select_playback_plan_with_eligibility,
    GenreOption, MoodOption, PlaybackCandidate, PlaybackEligibility, PlaybackSelection,
    PlaybackSelectionInput,
};

#[cfg(test)]
mod tests;
