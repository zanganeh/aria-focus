use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::private_beta::{PrivateBetaTrust, TRUST};
use audio_engine::{
    decode_track_with_limit, AuthoredRegion, AuthoredRegionKind, DecodeExpectation, DecodedProgram,
    MediaCodec, SourceLabel, MAX_PROGRAM_SAMPLES,
};
use catalogue::{
    available_genres_with_eligibility, available_moods_with_eligibility, canonical_pack_path,
    is_stable_identifier, select_playback_plan_for_item_with_eligibility,
    select_playback_plan_with_eligibility, stage_pack, verify_bundled_owner_waived_pack,
    verify_generated_local_pack, verify_installed_pack, AssetCodec, ContentPackManifest,
    GeneratedLocalRecord, GenreOption, ImportLimits, MoodOption, PlaybackEligibility,
    PlaybackSelectionInput, SafeRegionKind,
};
use domain::{Activity, TrackEnjoyment, TrackFeedback};
use persistence::{
    CatalogueRegistry, GeneratedLocalCustomerRecord, GeneratedLocalEvidenceRecord,
    GenrePreferenceStore, InstalledPackRecord, ItemFeedbackStore, MoodPreferenceStore,
    PackRegistration, PersistenceError, RegisteredItem, RegisteredTaxonomyTerm,
};
use serde::{Deserialize, Serialize};

const VALIDATED_STATUS: &str = "validated_metadata";
const OWNER_WAIVED_BUNDLED_STATUS: &str = "owner_waived_bundled_private_beta";
const GENERATED_LOCAL_STATUS: &str = "generated_local";
const RETIRED_PRIVATE_BETA_PACK_IDS: &[&str] = &["local-activity-library-v1"];
const RECEIPT_FORMAT_VERSION: u32 = 1;
const MAX_RECEIPT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallReceipt {
    format_version: u32,
    registration: PackRegistration,
    #[serde(default)]
    customer: Option<GeneratedLocalCustomerRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteReceipt {
    format_version: u32,
    pack_id: String,
    item_id: String,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PackSummary {
    pub id: String,
    pub title: String,
    pub version: String,
    pub item_count: u32,
    pub status: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PackServiceError {
    #[error(transparent)]
    Import(#[from] catalogue::PackImportError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("content pack {0} is already installed; overwrite and downgrade are not allowed")]
    AlreadyInstalled(String),
    #[error("content item IDs already exist: {ids}", ids = .0.join(", "))]
    ItemCollision(Vec<String>),
    #[error("installed pack registry is corrupt for {pack_id}: {reason}")]
    CorruptRegistry { pack_id: String, reason: String },
    #[error("pack files were installed but registration failed ({database}); filesystem rollback also failed ({rollback})")]
    RegistrationAndRollback {
        database: PersistenceError,
        rollback: std::io::Error,
    },
    #[error("content-pack recovery requires attention for {pack_id}: {reason}")]
    Recovery { pack_id: String, reason: String },
    #[error("installed audio could not be prepared: {0}")]
    Audio(String),
}

pub(crate) struct PreparedPackPlayback {
    pub(crate) program: DecodedProgram,
    pub(crate) primary_item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ItemFeedbackState {
    pub(crate) item_id: String,
    pub(crate) activity: Activity,
    pub(crate) focus_feedback: Option<TrackFeedback>,
    pub(crate) enjoyment: Option<TrackEnjoyment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FavoriteLibraryItem {
    pub(crate) item_id: String,
    pub(crate) activity: Activity,
    pub(crate) title: String,
    pub(crate) genre: Vec<String>,
    pub(crate) moods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MyMusicItem {
    pub(crate) item_id: String,
    pub(crate) title: String,
    pub(crate) duration_seconds: u16,
    pub(crate) created_at: i64,
    pub(crate) activity: Activity,
    pub(crate) job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ActivityGenreState {
    pub(crate) selected_genre_id: Option<String>,
    pub(crate) available_genres: Vec<GenreOptionDto>,
    pub(crate) selected_genre_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ActivityMoodState {
    pub(crate) selected_mood_id: Option<String>,
    pub(crate) available_moods: Vec<MoodOptionDto>,
    pub(crate) selected_mood_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct GenreOptionDto {
    pub(crate) id: String,
    pub(crate) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MoodOptionDto {
    pub(crate) id: String,
    pub(crate) label: String,
}

pub(crate) struct PackService<R: CatalogueRegistry> {
    registry: R,
    content_root: PathBuf,
    limits: ImportLimits,
    recent_item_id: Option<String>,
    resource_dir: Option<PathBuf>,
    private_beta_enabled: bool,
}

impl<R: CatalogueRegistry> PackService<R> {
    pub(crate) fn new(registry: R, content_root: PathBuf) -> Self {
        Self {
            registry,
            content_root,
            limits: ImportLimits::default(),
            recent_item_id: None,
            resource_dir: None,
            private_beta_enabled: false,
        }
    }

    pub(crate) fn with_resource_dir(mut self, resource_dir: Option<PathBuf>) -> Self {
        self.resource_dir = resource_dir;
        self.private_beta_enabled = true;
        self
    }

    #[cfg(test)]
    fn with_limits(registry: R, content_root: PathBuf, limits: ImportLimits) -> Self {
        Self {
            registry,
            content_root,
            limits,
            recent_item_id: None,
            resource_dir: None,
            private_beta_enabled: false,
        }
    }

    pub(crate) fn list(&mut self) -> Result<Vec<PackSummary>, PackServiceError> {
        self.install_bundled_private_beta()?;
        self.validated_records().map(|records| {
            records
                .iter()
                .map(|(record, _)| summary_for(record))
                .collect()
        })
    }

    /// Revalidate every registry row and installed tree before deterministic
    /// selection, then decode the selected bounded program off the callback.
    pub(crate) fn prepare_playback(
        &mut self,
        activity: Activity,
        genre_id: Option<&str>,
        mood_id: Option<&str>,
    ) -> Result<Option<PreparedPackPlayback>, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.install_bundled_private_beta()?;
        let records = self.validated_records()?;
        let manifests = records
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        let item_ids = manifests
            .iter()
            .flat_map(|manifest| manifest.items.iter().map(|item| item.id.clone()))
            .collect::<Vec<_>>();
        let item_feedback = self.registry.load_item_feedback(activity, &item_ids)?;
        let eligibility = self.private_beta_eligibility(&records);
        let Some(selection) = select_playback_plan_with_eligibility(
            &manifests,
            PlaybackSelectionInput {
                activity,
                genre_id,
                mood_id,
                previous_item_id: self.recent_item_id.as_deref(),
                item_feedback: &item_feedback,
            },
            &eligibility,
        ) else {
            return Ok(None);
        };
        let primary_item_id = selection.primary().item_id.clone();
        self.decode_selection(&manifests, selection.tracks, primary_item_id)
            .map(Some)
    }

    /// Revalidates the installed catalogue and decodes the exact requested
    /// favourite. This path deliberately does not call profile selection.
    pub(crate) fn prepare_favorite_playback(
        &mut self,
        activity: Activity,
        item_id: &str,
    ) -> Result<PreparedPackPlayback, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.install_bundled_private_beta()?;
        let records = self.validated_records()?;
        let requested = [item_id.to_owned()];
        let enjoyment = self.registry.load_item_enjoyment(activity, &requested)?;
        if enjoyment.get(item_id) != Some(&TrackEnjoyment::Liked) {
            return Err(PackServiceError::Audio(format!(
                "The favourite '{item_id}' is not liked for this activity."
            )));
        }
        let manifests = records
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        let selection = select_playback_plan_for_item_with_eligibility(
            &manifests,
            activity,
            item_id,
            &self.private_beta_eligibility(&records),
        )
        .ok_or_else(|| {
            PackServiceError::Audio(format!(
            "The favourite '{item_id}' is no longer a playable installed track for this activity."
        ))
        })?;
        let primary_item_id = selection.primary().item_id.clone();
        self.decode_selection(&manifests, selection.tracks, primary_item_id)
    }

    pub(crate) fn prepare_my_music_playback(
        &mut self,
        activity: Activity,
        item_id: &str,
    ) -> Result<PreparedPackPlayback, PackServiceError> {
        let records = self.validated_records()?;
        if !self
            .registry
            .list_generated_local_customers()?
            .iter()
            .any(|record| record.item_id == item_id && record.activity == activity)
        {
            return Err(PackServiceError::Audio(
                "That music is no longer available.".into(),
            ));
        }
        let manifests = records
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        let selection = select_playback_plan_for_item_with_eligibility(
            &manifests,
            activity,
            item_id,
            &self.private_beta_eligibility(&records),
        )
        .ok_or_else(|| PackServiceError::Audio("That music is no longer playable.".into()))?;
        let primary_item_id = selection.primary().item_id.clone();
        self.decode_selection(&manifests, selection.tracks, primary_item_id)
    }

    fn decode_selection(
        &self,
        manifests: &[ContentPackManifest],
        tracks: Vec<catalogue::PlaybackCandidate>,
        primary_item_id: String,
    ) -> Result<PreparedPackPlayback, PackServiceError> {
        let mut decoded = Vec::with_capacity(tracks.len());
        let mut remaining_samples = MAX_PROGRAM_SAMPLES;
        for candidate in &tracks {
            let manifest = manifests
                .iter()
                .find(|manifest| manifest.pack.id == candidate.pack_id)
                .ok_or_else(|| PackServiceError::Audio("selected pack disappeared".to_owned()))?;
            let asset_path = canonical_pack_path(&candidate.asset.path).ok_or_else(|| {
                PackServiceError::Audio("selected asset path is invalid".to_owned())
            })?;
            let expectation = DecodeExpectation {
                path: self.expected_install_path(manifest).join(asset_path),
                codec: match candidate.asset.codec {
                    AssetCodec::Wav => MediaCodec::Wav,
                    AssetCodec::Flac => MediaCodec::Flac,
                    AssetCodec::Mp3 => MediaCodec::Mp3,
                    AssetCodec::OggOpus => MediaCodec::OggOpus,
                },
                bytes: candidate.asset.bytes,
                sha256: candidate.asset.sha256.clone(),
                sample_rate_hz: candidate.asset.sample_rate_hz,
                channels: candidate.asset.channels,
                bit_depth: candidate.asset.bit_depth,
                duration_seconds: candidate.duration_seconds,
                regions: candidate
                    .safe_regions
                    .iter()
                    .map(|region| AuthoredRegion {
                        kind: match region.kind {
                            SafeRegionKind::Loop => AuthoredRegionKind::Loop,
                            SafeRegionKind::Crossfade => AuthoredRegionKind::Crossfade,
                        },
                        start_seconds: region.start_seconds,
                        end_seconds: region.end_seconds,
                    })
                    .collect(),
                label: SourceLabel {
                    pack_id: candidate.pack_id.clone(),
                    pack_title: candidate.pack_title.clone(),
                    item_id: candidate.item_id.clone(),
                    item_title: candidate.item_title.clone(),
                    variant_id: candidate.variant_id.clone(),
                },
            };
            let track = decode_track_with_limit(&expectation, remaining_samples)
                .map_err(|error| PackServiceError::Audio(error.to_string()))?;
            remaining_samples = remaining_samples
                .checked_sub(track.samples.len())
                .ok_or_else(|| {
                    PackServiceError::Audio("playback sample limit exhausted".to_owned())
                })?;
            decoded.push(track);
        }
        let program = DecodedProgram::new(decoded)
            .map_err(|error| PackServiceError::Audio(error.to_string()))?;
        Ok(PreparedPackPlayback {
            program,
            primary_item_id,
        })
    }

    pub(crate) fn favorites(&mut self) -> Result<Vec<FavoriteLibraryItem>, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.install_bundled_private_beta()?;
        let records = self.validated_records()?;
        let manifests = records
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        let eligibility = self.private_beta_eligibility(&records);
        let item_ids = records
            .iter()
            .flat_map(|(_, manifest)| manifest.items.iter().map(|item| item.id.clone()))
            .collect::<Vec<_>>();
        let mut favorites = Vec::new();
        for activity in [
            Activity::DeepWork,
            Activity::Motivation,
            Activity::Creativity,
            Activity::Learning,
            Activity::LightWork,
        ] {
            let liked = self.registry.load_item_enjoyment(activity, &item_ids)?;
            for (_, manifest) in &records {
                for item in &manifest.items {
                    if liked.get(&item.id) != Some(&TrackEnjoyment::Liked) {
                        continue;
                    }
                    if select_playback_plan_for_item_with_eligibility(
                        &manifests,
                        activity,
                        &item.id,
                        &eligibility,
                    )
                    .is_none()
                    {
                        continue;
                    }
                    let labels = |ids: &[String], terms: &[catalogue::TaxonomyTerm]| {
                        let mut labels = ids
                            .iter()
                            .filter_map(|id| {
                                terms
                                    .iter()
                                    .find(|term| &term.id == id)
                                    .map(|term| term.label.clone())
                            })
                            .collect::<Vec<_>>();
                        labels.sort();
                        labels
                    };
                    favorites.push(FavoriteLibraryItem {
                        item_id: item.id.clone(),
                        activity,
                        title: item.title.clone(),
                        genre: labels(&item.genre_ids, &manifest.taxonomy.genres),
                        moods: labels(&item.mood_ids, &manifest.taxonomy.moods),
                    });
                }
            }
        }
        favorites.sort_by(|left, right| {
            (left.activity.storage_key(), &left.title, &left.item_id).cmp(&(
                right.activity.storage_key(),
                &right.title,
                &right.item_id,
            ))
        });
        Ok(favorites)
    }

    pub(crate) fn remove_favorite(
        &mut self,
        activity: Activity,
        item_id: &str,
    ) -> Result<(), PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.ensure_validated_item(item_id)?;
        self.registry.clear_item_enjoyment(item_id, activity)?;
        Ok(())
    }

    pub(crate) fn commit_playback(&mut self, primary_item_id: String) {
        self.recent_item_id = Some(primary_item_id);
    }

    pub(crate) fn feedback_state(
        &mut self,
        activity: Activity,
        item_id: &str,
    ) -> Result<ItemFeedbackState, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.ensure_validated_item(item_id)?;
        let focus_feedback = self
            .registry
            .load_item_feedback(activity, &[item_id.to_owned()])?
            .remove(item_id);
        let enjoyment = self
            .registry
            .load_item_enjoyment(activity, &[item_id.to_owned()])?
            .remove(item_id);
        Ok(ItemFeedbackState {
            item_id: item_id.to_owned(),
            activity,
            focus_feedback,
            enjoyment,
        })
    }

    /// Feedback is limited to the installed item currently shown by the audio
    /// source, or the one most recently committed playback. The latter keeps
    /// feedback available immediately after Stop without accepting arbitrary
    /// installed catalogue items from a stale UI.
    pub(crate) fn feedback_state_for_displayed_item(
        &mut self,
        activity: Activity,
        item_id: &str,
        current_source: &SourceLabel,
    ) -> Result<ItemFeedbackState, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.ensure_feedback_item_is_displayed_or_recent(item_id, current_source)?;
        self.feedback_state(activity, item_id)
    }

    pub(crate) fn save_feedback(
        &mut self,
        activity: Activity,
        item_id: &str,
        focus_feedback: Option<TrackFeedback>,
        enjoyment: Option<TrackEnjoyment>,
    ) -> Result<ItemFeedbackState, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.ensure_validated_item(item_id)?;
        match focus_feedback {
            Some(feedback) => self
                .registry
                .save_item_feedback(item_id, activity, feedback)?,
            None => self.registry.clear_item_feedback(item_id, activity)?,
        }
        match enjoyment {
            Some(enjoyment) => self
                .registry
                .save_item_enjoyment(item_id, activity, enjoyment)?,
            None => self.registry.clear_item_enjoyment(item_id, activity)?,
        }
        self.feedback_state(activity, item_id)
    }

    pub(crate) fn save_feedback_for_displayed_item(
        &mut self,
        activity: Activity,
        item_id: &str,
        focus_feedback: Option<TrackFeedback>,
        enjoyment: Option<TrackEnjoyment>,
        current_source: &SourceLabel,
    ) -> Result<ItemFeedbackState, PackServiceError>
    where
        R: ItemFeedbackStore,
    {
        self.ensure_feedback_item_is_displayed_or_recent(item_id, current_source)?;
        self.save_feedback(activity, item_id, focus_feedback, enjoyment)
    }

    fn ensure_feedback_item_is_displayed_or_recent(
        &mut self,
        item_id: &str,
        current_source: &SourceLabel,
    ) -> Result<(), PackServiceError> {
        self.ensure_validated_item(item_id)?;
        let is_current_installed_item =
            current_source.pack_id != "bundled-test-source" && current_source.item_id == item_id;
        if is_current_installed_item || self.recent_item_id.as_deref() == Some(item_id) {
            Ok(())
        } else {
            Err(PackServiceError::Audio(
                "Feedback is available only for the displayed track or the track most recently played. Refresh the source and try again."
                    .to_owned(),
            ))
        }
    }

    fn ensure_validated_item(&mut self, item_id: &str) -> Result<(), PackServiceError> {
        if !is_stable_identifier(item_id)
            || !self
                .validated_records()?
                .iter()
                .any(|(_, manifest)| manifest.items.iter().any(|item| item.id == item_id))
        {
            return Err(PackServiceError::Audio(format!(
                "The installed track '{item_id}' is unavailable. Refresh or reinstall its content pack before saving feedback."
            )));
        }
        Ok(())
    }

    fn validated_records(
        &mut self,
    ) -> Result<Vec<(InstalledPackRecord, ContentPackManifest)>, PackServiceError> {
        self.reconcile_receipts()?;
        let records = self.registry.list_installed_packs()?;
        let validated = records
            .iter()
            // Retired owner-waived bundles remain registered so their directory
            // is still accounted for by the closed-world audit below, but they
            // must not participate in current-version validation or playback.
            // Their compatibility range is intentionally historical.
            // Keep this ordering aligned with docs/content-pack-upgrades.md.
            .filter(|record| !is_retired_private_beta(record))
            .map(|record| {
                self.validate_record_manifest(record)
                    .map(|manifest| (record.clone(), manifest))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.audit_pack_directories(&records)?;
        Ok(validated)
    }

    fn private_beta_eligibility(
        &self,
        records: &[(InstalledPackRecord, ContentPackManifest)],
    ) -> PlaybackEligibility {
        let eligibility = PlaybackEligibility::published_only().allowing_app_generated_items(
            records
                .iter()
                .filter(|(record, _)| record.status == GENERATED_LOCAL_STATUS)
                .flat_map(|(_, manifest)| manifest.items.iter().map(|item| item.id.clone())),
        );
        if !self.private_beta_enabled {
            return eligibility;
        }
        let Some(trust) = TRUST else {
            return eligibility;
        };
        let installed = records.iter().any(|(record, manifest)| {
            record.status == OWNER_WAIVED_BUNDLED_STATUS
                && record.pack_id == trust.pack_id
                && record.version == trust.version
                && record.manifest_sha256 == trust.manifest_sha256
                && manifest
                    .items
                    .iter()
                    .map(|item| item.id.as_str())
                    .eq(trust.item_ids.iter().copied())
        });
        if installed {
            eligibility.allowing_owner_waived_bundled_items(
                trust.item_ids.iter().map(|id| (*id).to_owned()),
            )
        } else {
            eligibility
        }
    }

    /// Installs only the resource pinned into this executable. It never reads
    /// an archive and never grants eligibility from a manifest field.
    fn install_bundled_private_beta(&mut self) -> Result<(), PackServiceError> {
        if !self.private_beta_enabled {
            return Ok(());
        }
        let Some(trust) = TRUST else {
            return Ok(());
        };
        let resource = self.resource_dir.as_ref().ok_or_else(|| PackServiceError::Recovery {
            pack_id: trust.pack_id.to_owned(),
            reason: "this build pins private-beta content but its bundled resource directory is unavailable".to_owned(),
        })?.join("private-beta-pack");
        let source_manifest = if trust.published {
            verify_installed_pack(&resource, trust.manifest_sha256)
        } else {
            verify_bundled_owner_waived_pack(&resource, trust.manifest_sha256)
        }
        .map_err(|error| {
            recovery_error(
                trust.pack_id,
                format!("bundled private-beta resource failed integrity verification: {error}"),
            )
        })?;
        self.verify_private_beta_trust(&source_manifest, trust)?;
        let target = self.expected_install_path(&source_manifest);
        let registration = registration_from_manifest_with_status(
            &source_manifest,
            &target,
            String::from_utf8(
                catalogue::canonical_manifest_bytes(&source_manifest)
                    .map_err(|error| recovery_error(trust.pack_id, error.to_string()))?,
            )
            .map_err(|error| recovery_error(trust.pack_id, error.to_string()))?,
            trust.manifest_sha256.to_owned(),
            trust.bundle_sha256.to_owned(),
            if trust.published {
                VALIDATED_STATUS
            } else {
                OWNER_WAIVED_BUNDLED_STATUS
            },
        );
        if let Some(record) = self.registry.find_installed_pack(trust.pack_id)? {
            if !trust.published {
                if record.status != OWNER_WAIVED_BUNDLED_STATUS {
                    return Err(registry_preflight_error(
                        trust.pack_id,
                        "listening-test library pack ID is occupied by a different registry record",
                    ));
                }
                if record.version == trust.version
                    && record.manifest_sha256 == trust.manifest_sha256
                    && record.archive_sha256 == trust.bundle_sha256
                {
                    self.validate_record_manifest(&record)?;
                    return Ok(());
                }
                // A new local listening-test build may intentionally replace the
                // previous owner-waived bundle (for example FLAC -> Ogg Opus).
                // The replacement is only allowed when every track identity is
                // preserved, so user feedback remains attached to the same items.
                self.upgrade_owner_waived_bundled_library(
                    &record,
                    &source_manifest,
                    &resource,
                    &target,
                    &registration,
                    trust,
                )?;
                return Ok(());
            }
            if record.status == VALIDATED_STATUS {
                if record.version != trust.version
                    || record.manifest_sha256 != trust.manifest_sha256
                    || record.archive_sha256 != trust.bundle_sha256
                {
                    return Err(registry_preflight_error(
                        trust.pack_id,
                        "bundled library registry record differs from this build",
                    ));
                }
                self.validate_record_manifest(&record)?;
                self.cleanup_superseded_owner_waived(&source_manifest, &target)?;
                return Ok(());
            }
            if record.status != OWNER_WAIVED_BUNDLED_STATUS {
                return Err(registry_preflight_error(
                    trust.pack_id,
                    "bundled library pack ID is occupied by a different registry record",
                ));
            }
            self.upgrade_owner_waived_bundled_library(
                &record,
                &source_manifest,
                &resource,
                &target,
                &registration,
                trust,
            )?;
            return Ok(());
        }
        if target.exists() {
            return Err(recovery_error(trust.pack_id, "private-beta install target already exists without its expected registry record; it was not overwritten"));
        }
        let item_ids = source_manifest
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let collisions = self.registry.find_existing_item_ids(&item_ids)?;
        if !collisions.is_empty() {
            return Err(PackServiceError::ItemCollision(collisions));
        }
        let parent = target
            .parent()
            .ok_or_else(|| recovery_error(trust.pack_id, "private-beta target has no parent"))?;
        fs::create_dir_all(parent).map_err(|error| {
            recovery_error(
                trust.pack_id,
                format!("cannot create private-beta install parent: {error}"),
            )
        })?;
        ensure_plain_directory(parent, "private-beta install parent")?;
        let staging = tempfile::Builder::new()
            .prefix(".private-beta-")
            .tempdir_in(parent)
            .map_err(|error| {
                recovery_error(
                    trust.pack_id,
                    format!("cannot create private-beta staging directory: {error}"),
                )
            })?;
        copy_private_beta_tree(&resource, staging.path(), &source_manifest)?;
        let staged_verification = if trust.published {
            verify_installed_pack(staging.path(), trust.manifest_sha256)
        } else {
            verify_bundled_owner_waived_pack(staging.path(), trust.manifest_sha256)
        };
        staged_verification.map_err(|error| {
            recovery_error(
                trust.pack_id,
                format!("staged private-beta resource failed integrity verification: {error}"),
            )
        })?;
        fs::rename(staging.path(), &target).map_err(|error| {
            recovery_error(
                trust.pack_id,
                format!("cannot atomically install private-beta resource: {error}"),
            )
        })?;
        if let Err(error) = self.registry.register_pack(&registration) {
            if let Err(rollback) = fs::remove_dir_all(&target) {
                return Err(PackServiceError::RegistrationAndRollback {
                    database: error,
                    rollback,
                });
            }
            return Err(error.into());
        }
        Ok(())
    }

    fn upgrade_owner_waived_bundled_library(
        &mut self,
        record: &InstalledPackRecord,
        source_manifest: &ContentPackManifest,
        resource: &Path,
        target: &Path,
        registration: &PackRegistration,
        trust: PrivateBetaTrust,
    ) -> Result<(), PackServiceError> {
        let legacy: ContentPackManifest = serde_json::from_str(&record.canonical_manifest)
            .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        legacy
            .validate_bundled_owner_waived()
            .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        let legacy_canonical = catalogue::canonical_manifest_bytes(&legacy)
            .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        let legacy_path = self.expected_install_path(&legacy);
        if legacy_canonical != record.canonical_manifest.as_bytes()
            || catalogue::import::hash_bytes(&legacy_canonical) != record.manifest_sha256
            || record.pack_id != legacy.pack.id
            || record.version != legacy.pack.version
            || record.title != legacy.pack.title
            || record.item_count as usize != legacy.items.len()
            || Path::new(&record.install_path) != legacy_path
            || legacy.pack.id != source_manifest.pack.id
            || legacy
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<HashSet<_>>()
                != source_manifest
                    .items
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<HashSet<_>>()
            || legacy_path == target
        {
            return Err(registry_preflight_error(
                &record.pack_id,
                "legacy library cannot be upgraded without changing track identity",
            ));
        }
        verify_bundled_owner_waived_pack(&legacy_path, &record.manifest_sha256).map_err(
            |error| {
                recovery_error(
                    &record.pack_id,
                    format!("legacy library is corrupt: {error}"),
                )
            },
        )?;
        let created_target = if target.exists() {
            verify_trusted_bundled_pack(target, trust).map_err(|error| {
                recovery_error(
                    &record.pack_id,
                    format!("interrupted library upgrade target is corrupt: {error}"),
                )
            })?;
            false
        } else {
            let parent = target.parent().ok_or_else(|| {
                recovery_error(&record.pack_id, "bundled library target has no parent")
            })?;
            fs::create_dir_all(parent).map_err(|error| {
                recovery_error(
                    &record.pack_id,
                    format!("cannot create upgrade parent: {error}"),
                )
            })?;
            let staging = tempfile::Builder::new()
                .prefix(".bundled-upgrade-")
                .tempdir_in(parent)
                .map_err(|error| {
                    recovery_error(
                        &record.pack_id,
                        format!("cannot stage library upgrade: {error}"),
                    )
                })?;
            copy_private_beta_tree(resource, staging.path(), source_manifest)?;
            verify_trusted_bundled_pack(staging.path(), trust).map_err(|error| {
                recovery_error(
                    &record.pack_id,
                    format!("staged library upgrade is corrupt: {error}"),
                )
            })?;
            fs::rename(staging.path(), target).map_err(|error| {
                recovery_error(
                    &record.pack_id,
                    format!("cannot install library upgrade: {error}"),
                )
            })?;
            true
        };
        if let Err(database) = self
            .registry
            .replace_owner_waived_pack_preserving_feedback(registration)
        {
            if created_target {
                if let Err(rollback) = fs::remove_dir_all(target) {
                    return Err(PackServiceError::RegistrationAndRollback { database, rollback });
                }
            }
            return Err(database.into());
        }
        self.cleanup_superseded_owner_waived(source_manifest, target)
    }

    fn cleanup_superseded_owner_waived(
        &self,
        published: &ContentPackManifest,
        current: &Path,
    ) -> Result<(), PackServiceError> {
        let Some(parent) = current.parent() else {
            return Err(recovery_error(
                &published.pack.id,
                "library target has no parent",
            ));
        };
        if !parent.exists() {
            return Ok(());
        }
        let expected_ids = published
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        for entry in fs::read_dir(parent).map_err(|error| {
            recovery_error(
                &published.pack.id,
                format!("cannot inspect library versions: {error}"),
            )
        })? {
            let path = entry
                .map_err(|error| recovery_error(&published.pack.id, error.to_string()))?
                .path();
            if path == current {
                continue;
            }
            let manifest_path = path.join(catalogue::MANIFEST_PATH);
            let hash = catalogue::hash_file_sha256(&manifest_path).map_err(|error| {
                recovery_error(
                    &published.pack.id,
                    format!("cannot verify superseded library: {error}"),
                )
            })?;
            let legacy = verify_bundled_owner_waived_pack(&path, &hash).map_err(|error| {
                recovery_error(
                    &published.pack.id,
                    format!("unrecognized superseded library was preserved: {error}"),
                )
            })?;
            let legacy_ids = legacy
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<HashSet<_>>();
            if legacy.pack.id != published.pack.id || legacy_ids != expected_ids {
                return Err(recovery_error(
                    &published.pack.id,
                    "superseded library changes track identity and was preserved",
                ));
            }
            fs::remove_dir_all(&path).map_err(|error| {
                recovery_error(
                    &published.pack.id,
                    format!("cannot remove verified superseded library: {error}"),
                )
            })?;
        }
        Ok(())
    }

    fn verify_private_beta_trust(
        &self,
        manifest: &ContentPackManifest,
        trust: PrivateBetaTrust,
    ) -> Result<(), PackServiceError> {
        // The canonical manifest hash is checked before this method is called,
        // and `verify_installed_pack` verifies every declared asset
        // against that manifest. Do not repeat those checks with a separately
        // derived bundle value: it has no additional security value and can
        // diverge across otherwise identical build/runtime representations.
        let _ = manifest;
        let _ = trust;
        Ok(())
    }

    pub(crate) fn import(&mut self, archive_path: &Path) -> Result<PackSummary, PackServiceError> {
        self.reconcile_receipts()?;
        let staging_root = self.content_root.join("staging");
        let staged = stage_pack(archive_path, &staging_root, self.limits)?;
        staged
            .manifest
            .validate_app_compatibility(env!("CARGO_PKG_VERSION"))
            .map_err(catalogue::PackImportError::from)?;
        let pack_id = staged.manifest.pack.id.clone();
        if self.registry.find_installed_pack(&pack_id)?.is_some() {
            return Err(PackServiceError::AlreadyInstalled(pack_id));
        }
        let item_ids = staged
            .manifest
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let collisions = self.registry.find_existing_item_ids(&item_ids)?;
        if !collisions.is_empty() {
            return Err(PackServiceError::ItemCollision(collisions));
        }

        let target = self.expected_install_path(&staged.manifest);
        let registration = registration_for(&staged.manifest, &target, &staged)?;
        let summary = summary_for(&registration.pack);
        let receipt = InstallReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            registration: registration.clone(),
            customer: None,
        };
        let receipt_path = self.write_receipt(&receipt)?;
        let installed = match staged.install_to(&target) {
            Ok(installed) => installed,
            Err(error) => {
                if target.exists() {
                    return Err(PackServiceError::Recovery {
                        pack_id,
                        reason: format!(
                            "install target exists after a post-rename failure ({error}); durable receipt preserved for startup reconciliation"
                        ),
                    });
                }
                if let Err(cleanup) = self.remove_receipt(&receipt_path) {
                    return Err(PackServiceError::Recovery {
                        pack_id,
                        reason: format!(
                            "file installation failed ({error}); receipt cleanup failed ({cleanup})"
                        ),
                    });
                }
                return Err(error.into());
            }
        };
        debug_assert_eq!(installed.directory, target);
        if let Err(database) = self.registry.register_pack(&registration) {
            if let Err(rollback) = fs::remove_dir_all(&target) {
                return Err(PackServiceError::RegistrationAndRollback { database, rollback });
            }
            cleanup_empty_install_parents(&target, &self.content_root.join("packs"));
            if let Err(cleanup) = self.remove_receipt(&receipt_path) {
                return Err(PackServiceError::Recovery {
                    pack_id,
                    reason: format!(
                        "database registration failed ({database}); files were rolled back but receipt cleanup failed ({cleanup})"
                    ),
                });
            }
            return Err(database.into());
        }
        self.remove_receipt(&receipt_path)
            .map_err(|error| PackServiceError::Recovery {
                pack_id,
                reason: format!(
                    "database registration committed, but its stale receipt could not be removed: {error}"
                ),
            })?;
        Ok(summary)
    }

    /// Saves only an app-produced local generation. `staging_directory` must
    /// be the exact per-job directory below the service-owned Studio staging
    /// root; arbitrary archives and caller-selected paths cannot enter here.
    ///
    /// The next Studio save command consumes this internal staging API. Until
    /// that command exists, it intentionally has no production caller.
    #[allow(dead_code)]
    pub(crate) fn install_generated_local(
        &mut self,
        record: GeneratedLocalRecord,
        customer: GeneratedLocalCustomerRecord,
        staging_directory: &Path,
    ) -> Result<PackSummary, PackServiceError> {
        self.reconcile_receipts()?;
        record
            .validate()
            .map_err(catalogue::PackImportError::from)?;
        let expected_staging = self
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        if staging_directory != expected_staging || !staging_directory.is_dir() {
            return Err(PackServiceError::Recovery {
                pack_id: record.manifest.pack.id.clone(),
                reason: "generated-local source is not the controlled per-job staging directory"
                    .to_owned(),
            });
        }
        let pack_id = record.manifest.pack.id.clone();
        let canonical = catalogue::canonical_manifest_bytes(&record.manifest)
            .map_err(catalogue::PackImportError::from)?;
        fs::write(staging_directory.join("manifest.json"), &canonical)
            .map_err(catalogue::PackImportError::from)?;
        let manifest_hash = catalogue::import::hash_bytes(&canonical);
        // Verify the exact staged tree before making a durable receipt. This
        // rejects links, traversal-shaped paths, unexpected files and any
        // declared technical/hash mismatch.
        verify_generated_local_pack(staging_directory, &manifest_hash)?;
        let evidence_hash =
            catalogue::import::hash_bytes(&serde_json::to_vec(&record.evidence).map_err(|e| {
                PackServiceError::Recovery {
                    pack_id: pack_id.clone(),
                    reason: e.to_string(),
                }
            })?);
        let target = self.expected_install_path(&record.manifest);
        let mut registration = registration_from_manifest_with_status(
            &record.manifest,
            &target,
            String::from_utf8(canonical).map_err(|e| PackServiceError::Recovery {
                pack_id: pack_id.clone(),
                reason: e.to_string(),
            })?,
            manifest_hash,
            evidence_hash,
            GENERATED_LOCAL_STATUS,
        );
        registration.generated_local_evidence = Some(GeneratedLocalEvidenceRecord {
            generation_job_id: record.generation_job_id.clone(),
            evidence_json: serde_json::to_string(&record.evidence).map_err(|error| {
                PackServiceError::Recovery {
                    pack_id: pack_id.clone(),
                    reason: error.to_string(),
                }
            })?,
            created_at_unix_seconds: record.evidence.completed_at_unix_seconds,
        });
        let receipt = InstallReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            registration: registration.clone(),
            customer: Some(customer.clone()),
        };
        if let Some(existing) = self.registry.find_installed_pack(&pack_id)? {
            if existing != registration.pack {
                return Err(PackServiceError::CorruptRegistry {
                    pack_id,
                    reason: "existing generated-local registration conflicts with this job".into(),
                });
            }
            self.validate_record_manifest(&existing)?;
            self.registry
                .register_generated_local_pack(&registration, &customer)?;
            fs::remove_dir_all(staging_directory).map_err(|error| {
                recovery_error(
                    &existing.pack_id,
                    format!("exact retry succeeded but staging cleanup failed: {error}"),
                )
            })?;
            return Ok(summary_for(&existing));
        }
        let receipt_path = self.write_receipt(&receipt)?;
        if target.exists() {
            return Err(PackServiceError::AlreadyInstalled(pack_id));
        }
        fs::create_dir_all(target.parent().unwrap_or(&self.content_root))
            .map_err(catalogue::PackImportError::from)?;
        if let Err(error) = fs::rename(staging_directory, &target) {
            let _ = self.remove_receipt(&receipt_path);
            return Err(catalogue::PackImportError::from(error).into());
        }
        if let Err(error) = self
            .registry
            .register_generated_local_pack(&registration, &customer)
        {
            return Err(PackServiceError::Recovery { pack_id, reason: format!("generated-local files retained with receipt after registration failure: {error}") });
        }
        self.remove_receipt(&receipt_path)
            .map_err(|error| PackServiceError::Recovery {
                pack_id: registration.pack.pack_id.clone(),
                reason: error.to_string(),
            })?;
        Ok(summary_for(&registration.pack))
    }

    pub(crate) fn generated_local_staging_path(
        &self,
        generation_job_id: &str,
    ) -> Result<PathBuf, PackServiceError> {
        if !is_stable_identifier(generation_job_id) {
            return Err(PackServiceError::Recovery {
                pack_id: "generated.local.invalid".into(),
                reason: "generation job identifier is invalid".into(),
            });
        }
        Ok(self
            .content_root
            .join("studio-staging")
            .join(generation_job_id))
    }

    pub(crate) fn list_my_music(&mut self) -> Result<Vec<MyMusicItem>, PackServiceError> {
        self.reconcile_receipts()?;
        let records = self.registry.list_generated_local_customers()?;
        let mut result = Vec::with_capacity(records.len());
        for record in records {
            let pack = self
                .registry
                .find_installed_pack(&record.pack_id)?
                .ok_or_else(|| PackServiceError::CorruptRegistry {
                    pack_id: record.pack_id.clone(),
                    reason: "customer metadata has no installed pack".into(),
                })?;
            let manifest = self.validate_record_manifest(&pack)?;
            let item = manifest
                .items
                .first()
                .ok_or_else(|| PackServiceError::CorruptRegistry {
                    pack_id: pack.pack_id.clone(),
                    reason: "manifest has no item".into(),
                })?;
            if pack.status != GENERATED_LOCAL_STATUS
                || manifest.items.len() != 1
                || item.id != record.item_id
                || !item
                    .activity_suitability
                    .iter()
                    .any(|entry| entry.activity == record.activity && entry.suitability > 0.0)
            {
                return Err(PackServiceError::CorruptRegistry {
                    pack_id: record.pack_id,
                    reason: "customer metadata differs from its generated-local manifest".into(),
                });
            }
            let job_id = record
                .pack_id
                .strip_prefix("generated.local.")
                .ok_or_else(|| PackServiceError::CorruptRegistry {
                    pack_id: record.pack_id.clone(),
                    reason: "generated pack identifier is invalid".into(),
                })?
                .to_owned();
            result.push(MyMusicItem {
                item_id: record.item_id,
                title: record.title,
                duration_seconds: item.analysis.duration_seconds.round() as u16,
                created_at: record.created_at_unix_seconds,
                activity: record.activity,
                job_id,
            });
        }
        Ok(result)
    }

    pub(crate) fn rename_my_music(
        &mut self,
        item_id: &str,
        title: &str,
    ) -> Result<(), PackServiceError> {
        self.registry
            .rename_generated_local_customer(item_id, title)?;
        Ok(())
    }

    pub(crate) fn delete_my_music(&mut self, item_id: &str) -> Result<bool, PackServiceError> {
        self.reconcile_delete_receipts()?;
        let Some(customer) = self
            .registry
            .list_generated_local_customers()?
            .into_iter()
            .find(|record| record.item_id == item_id)
        else {
            return Ok(false);
        };
        let record = self
            .registry
            .find_installed_pack(&customer.pack_id)?
            .ok_or_else(|| PackServiceError::CorruptRegistry {
                pack_id: customer.pack_id.clone(),
                reason: "customer metadata has no generated-local pack".into(),
            })?;
        let manifest = self.validate_record_manifest(&record)?;
        if record.status != GENERATED_LOCAL_STATUS
            || manifest.items.len() != 1
            || manifest.items[0].id != customer.item_id
        {
            return Err(PackServiceError::CorruptRegistry {
                pack_id: record.pack_id,
                reason: "customer metadata differs from its generated-local pack".into(),
            });
        }
        let root = self.content_root.join("packs");
        let path = self.expected_install_path(&manifest);
        if Path::new(&record.install_path) != path {
            return Err(recovery_error(
                &record.pack_id,
                "generated-local install path differs",
            ));
        }
        ensure_plain_directory(&root, "packs root")?;
        ensure_plain_directory(path.parent().unwrap_or(&root), "generated pack parent")?;
        ensure_plain_directory(&path, "generated pack target")?;
        let receipt = DeleteReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            pack_id: record.pack_id.clone(),
            item_id: customer.item_id,
            version: record.version.clone(),
        };
        let receipt_path = self.write_delete_receipt(&receipt)?;
        let trash_root = self.content_root.join("delete-trash");
        fs::create_dir_all(&trash_root).map_err(|error| {
            recovery_error(
                &record.pack_id,
                format!("cannot create delete trash: {error}"),
            )
        })?;
        ensure_plain_directory(&trash_root, "delete trash")?;
        let tombstone = trash_root.join(delete_receipt_file_name(&receipt));
        if tombstone.exists() {
            return Err(recovery_error(
                &record.pack_id,
                "delete tombstone already exists",
            ));
        }
        fs::rename(&path, &tombstone).map_err(|error| {
            recovery_error(
                &record.pack_id,
                format!("cannot stage generated music deletion: {error}"),
            )
        })?;
        let removed = match self.registry.unregister_generated_local(item_id) {
            Ok(record) => record,
            Err(database) => {
                if let Err(rollback) = fs::rename(&tombstone, &path) {
                    return Err(recovery_error(
                        &receipt.pack_id,
                        format!("delete database failed ({database}); filesystem restore also failed ({rollback})"),
                    ));
                }
                let _ = self.remove_receipt(&receipt_path);
                return Err(database.into());
            }
        };
        if removed.as_ref() != Some(&record) {
            return Err(recovery_error(
                &receipt.pack_id,
                "delete transaction returned a different registry record",
            ));
        }
        fs::remove_dir_all(&tombstone).map_err(|error| {
            recovery_error(
                &receipt.pack_id,
                format!("registry deletion committed; deferred trash cleanup failed: {error}"),
            )
        })?;
        self.remove_receipt(&receipt_path).map_err(|error| {
            recovery_error(
                &receipt.pack_id,
                format!("cannot remove delete receipt: {error}"),
            )
        })?;
        cleanup_empty_install_parents(&path, &root);
        Ok(true)
    }

    fn reconcile_receipts(&mut self) -> Result<(), PackServiceError> {
        self.reconcile_delete_receipts()?;
        let receipt_root = self.content_root.join("receipts");
        if !receipt_root.exists() {
            return Ok(());
        }
        ensure_plain_directory(&receipt_root, "receipt root")?;
        let mut paths = fs::read_dir(&receipt_root)
            .map_err(|error| recovery_error("unknown", format!("cannot read receipts: {error}")))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                recovery_error("unknown", format!("cannot enumerate receipts: {error}"))
            })?;
        paths.sort();
        for path in paths {
            let receipt = self.read_receipt(&path)?;
            let expected_name = receipt_file_name(
                &receipt.registration.pack.pack_id,
                &receipt.registration.pack.version,
            );
            if path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
                return Err(recovery_error(
                    &receipt.registration.pack.pack_id,
                    "receipt filename does not match its pack identity",
                ));
            }
            self.reconcile_receipt(&path, &receipt)?;
        }
        Ok(())
    }

    fn reconcile_delete_receipts(&mut self) -> Result<(), PackServiceError> {
        let root = self.content_root.join("delete-receipts");
        if !root.exists() {
            return Ok(());
        }
        ensure_plain_directory(&root, "delete receipt root")?;
        let mut paths = fs::read_dir(&root)
            .map_err(|error| {
                recovery_error("unknown", format!("cannot read delete receipts: {error}"))
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                recovery_error(
                    "unknown",
                    format!("cannot enumerate delete receipts: {error}"),
                )
            })?;
        paths.sort();
        for receipt_path in paths {
            let receipt = self.read_delete_receipt(&receipt_path)?;
            if receipt_path.file_name().and_then(|name| name.to_str())
                != Some(delete_receipt_file_name(&receipt).as_str())
                || receipt.format_version != RECEIPT_FORMAT_VERSION
                || !is_stable_identifier(&receipt.pack_id)
                || !is_stable_identifier(&receipt.item_id)
                || receipt.item_id != format!("{}.item", receipt.pack_id)
            {
                return Err(recovery_error(
                    &receipt.pack_id,
                    "delete receipt identity is invalid",
                ));
            }
            let target = self
                .content_root
                .join("packs")
                .join(&receipt.pack_id)
                .join(version_storage_key(&receipt.version));
            let tombstone = self
                .content_root
                .join("delete-trash")
                .join(delete_receipt_file_name(&receipt));
            let registered = self
                .registry
                .find_installed_pack(&receipt.pack_id)?
                .is_some();
            match (registered, target.exists(), tombstone.exists()) {
                (true, true, false) => {}
                (true, false, true) => fs::rename(&tombstone, &target).map_err(|error| {
                    recovery_error(
                        &receipt.pack_id,
                        format!("cannot restore interrupted delete: {error}"),
                    )
                })?,
                (false, false, true) => fs::remove_dir_all(&tombstone).map_err(|error| {
                    recovery_error(
                        &receipt.pack_id,
                        format!("cannot finish interrupted delete: {error}"),
                    )
                })?,
                (false, false, false) => {}
                _ => {
                    return Err(recovery_error(
                        &receipt.pack_id,
                        "delete receipt, registry, target, and tombstone disagree",
                    ))
                }
            }
            self.remove_receipt(&receipt_path).map_err(|error| {
                recovery_error(
                    &receipt.pack_id,
                    format!("cannot remove reconciled delete receipt: {error}"),
                )
            })?;
        }
        Ok(())
    }

    fn reconcile_receipt(
        &mut self,
        receipt_path: &Path,
        receipt: &InstallReceipt,
    ) -> Result<(), PackServiceError> {
        let pack_id = receipt.registration.pack.pack_id.clone();
        if receipt.format_version != RECEIPT_FORMAT_VERSION {
            return Err(recovery_error(&pack_id, "unsupported receipt version"));
        }
        let manifest: ContentPackManifest =
            serde_json::from_str(&receipt.registration.pack.canonical_manifest).map_err(
                |error| recovery_error(&pack_id, format!("receipt manifest is invalid: {error}")),
            )?;
        if receipt.registration.pack.status == GENERATED_LOCAL_STATUS {
            let job_id = manifest
                .pack
                .id
                .strip_prefix("generated.local.")
                .unwrap_or("");
            manifest.validate_generated_local(job_id)
        } else {
            manifest.validate_published()
        }
        .map_err(|error| recovery_error(&pack_id, error.to_string()))?;
        manifest
            .validate_app_compatibility(env!("CARGO_PKG_VERSION"))
            .map_err(|error| recovery_error(&pack_id, error.to_string()))?;
        let target = self.expected_install_path(&manifest);
        let canonical_manifest = String::from_utf8(
            catalogue::canonical_manifest_bytes(&manifest)
                .map_err(|error| recovery_error(&pack_id, error.to_string()))?,
        )
        .map_err(|error| recovery_error(&pack_id, error.to_string()))?;
        if !valid_sha256(&receipt.registration.pack.archive_sha256) {
            return Err(recovery_error(&pack_id, "receipt archive hash is invalid"));
        }
        let manifest_sha256 = catalogue::import::hash_bytes(canonical_manifest.as_bytes());
        let mut expected_registration = registration_from_manifest_with_status(
            &manifest,
            &target,
            canonical_manifest,
            manifest_sha256,
            receipt.registration.pack.archive_sha256.clone(),
            &receipt.registration.pack.status,
        );
        if receipt.registration.pack.status == GENERATED_LOCAL_STATUS {
            let evidence = receipt
                .registration
                .generated_local_evidence
                .as_ref()
                .ok_or_else(|| {
                    recovery_error(&pack_id, "generated-local receipt lacks evidence")
                })?;
            let parsed: catalogue::LocalGenerationEvidence =
                serde_json::from_str(&evidence.evidence_json).map_err(|error| {
                    recovery_error(
                        &pack_id,
                        format!("generated-local evidence is invalid: {error}"),
                    )
                })?;
            let generated = GeneratedLocalRecord {
                generation_job_id: evidence.generation_job_id.clone(),
                manifest: manifest.clone(),
                evidence: parsed,
            };
            generated
                .validate()
                .map_err(|error| recovery_error(&pack_id, error.to_string()))?;
            if evidence.created_at_unix_seconds != generated.evidence.completed_at_unix_seconds {
                return Err(recovery_error(
                    &pack_id,
                    "generated-local evidence timestamp differs",
                ));
            }
            expected_registration.generated_local_evidence = Some(evidence.clone());
        }
        if receipt.registration != expected_registration {
            return Err(recovery_error(
                &pack_id,
                "receipt transaction differs from its canonical manifest",
            ));
        }
        if !target.exists() {
            return Err(recovery_error(
                &pack_id,
                "durable receipt exists but its install target is missing; no files were deleted",
            ));
        }
        let verify = if receipt.registration.pack.status == GENERATED_LOCAL_STATUS {
            verify_generated_local_pack(&target, &receipt.registration.pack.manifest_sha256)
        } else {
            verify_installed_pack(&target, &receipt.registration.pack.manifest_sha256)
        };
        verify.map_err(|error| {
            recovery_error(
                &pack_id,
                format!("target is incomplete or corrupt: {error}"),
            )
        })?;

        if let Some(record) = self.registry.find_installed_pack(&pack_id)? {
            if record != receipt.registration.pack {
                return Err(recovery_error(
                    &pack_id,
                    "committed registry record differs from its stale receipt",
                ));
            }
            if record.status == GENERATED_LOCAL_STATUS {
                let customer = receipt.customer.as_ref().ok_or_else(|| {
                    recovery_error(&pack_id, "generated-local receipt lacks customer metadata")
                })?;
                self.registry
                    .register_generated_local_pack(&receipt.registration, customer)
                    .map_err(|error| {
                        recovery_error(
                            &pack_id,
                            format!("generated-local receipt conflicts with SQLite: {error}"),
                        )
                    })?;
            } else if receipt.customer.is_some() {
                return Err(recovery_error(
                    &pack_id,
                    "published receipt unexpectedly contains customer metadata",
                ));
            }
            self.remove_receipt(receipt_path).map_err(|error| {
                recovery_error(&pack_id, format!("cannot remove stale receipt: {error}"))
            })?;
            return Ok(());
        }

        let item_ids = receipt
            .registration
            .items
            .iter()
            .map(|item| item.item_id.clone())
            .collect::<Vec<_>>();
        let collisions = self.registry.find_existing_item_ids(&item_ids)?;
        if !collisions.is_empty() {
            return Err(recovery_error(
                &pack_id,
                format!(
                    "recovery item IDs collide with the registry: {}",
                    collisions.join(", ")
                ),
            ));
        }
        let registration_result = if receipt.registration.pack.status == GENERATED_LOCAL_STATUS {
            let customer = receipt.customer.as_ref().ok_or_else(|| {
                recovery_error(&pack_id, "generated-local receipt lacks customer metadata")
            })?;
            self.registry
                .register_generated_local_pack(&receipt.registration, customer)
        } else {
            if receipt.customer.is_some() {
                return Err(recovery_error(
                    &pack_id,
                    "published receipt unexpectedly contains customer metadata",
                ));
            }
            self.registry.register_pack(&receipt.registration)
        };
        registration_result.map_err(|error| {
            recovery_error(
                &pack_id,
                format!(
                    "verified target could not be finalized in SQLite: {error}; receipt preserved"
                ),
            )
        })?;
        self.remove_receipt(receipt_path).map_err(|error| {
            recovery_error(
                &pack_id,
                format!("registry committed but stale receipt removal failed: {error}"),
            )
        })?;
        Ok(())
    }

    fn write_receipt(&self, receipt: &InstallReceipt) -> Result<PathBuf, PackServiceError> {
        let pack_id = &receipt.registration.pack.pack_id;
        let root = self.content_root.join("receipts");
        fs::create_dir_all(&root).map_err(|error| {
            recovery_error(pack_id, format!("cannot create receipt root: {error}"))
        })?;
        ensure_plain_directory(&root, "receipt root")?;
        let path = root.join(receipt_file_name(
            pack_id,
            &receipt.registration.pack.version,
        ));
        let bytes = serde_json::to_vec(receipt).map_err(|error| {
            recovery_error(pack_id, format!("cannot serialize receipt: {error}"))
        })?;
        if bytes.len() as u64 > MAX_RECEIPT_BYTES {
            return Err(recovery_error(
                pack_id,
                "receipt exceeds its hard size limit",
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                recovery_error(pack_id, format!("cannot create durable receipt: {error}"))
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                recovery_error(pack_id, format!("cannot persist durable receipt: {error}"))
            })?;
        sync_parent_directory(&path).map_err(|error| {
            recovery_error(
                pack_id,
                format!(
                    "receipt file was synced but its directory entry could not be synced: {error}"
                ),
            )
        })?;
        Ok(path)
    }

    fn write_delete_receipt(&self, receipt: &DeleteReceipt) -> Result<PathBuf, PackServiceError> {
        let root = self.content_root.join("delete-receipts");
        fs::create_dir_all(&root).map_err(|error| {
            recovery_error(
                &receipt.pack_id,
                format!("cannot create delete receipt root: {error}"),
            )
        })?;
        ensure_plain_directory(&root, "delete receipt root")?;
        let path = root.join(delete_receipt_file_name(receipt));
        let bytes = serde_json::to_vec(receipt).map_err(|error| {
            recovery_error(
                &receipt.pack_id,
                format!("cannot serialize delete receipt: {error}"),
            )
        })?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                recovery_error(
                    &receipt.pack_id,
                    format!("cannot create delete receipt: {error}"),
                )
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                recovery_error(
                    &receipt.pack_id,
                    format!("cannot persist delete receipt: {error}"),
                )
            })?;
        sync_parent_directory(&path).map_err(|error| {
            recovery_error(
                &receipt.pack_id,
                format!("cannot sync delete receipt directory: {error}"),
            )
        })?;
        Ok(path)
    }

    fn read_delete_receipt(&self, path: &Path) -> Result<DeleteReceipt, PackServiceError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            recovery_error("unknown", format!("cannot inspect delete receipt: {error}"))
        })?;
        if is_link_or_reparse(&metadata)
            || !metadata.is_file()
            || metadata.len() > MAX_RECEIPT_BYTES
        {
            return Err(recovery_error(
                "unknown",
                "delete receipt is not a bounded plain file",
            ));
        }
        let bytes = fs::read(path).map_err(|error| {
            recovery_error("unknown", format!("cannot read delete receipt: {error}"))
        })?;
        serde_json::from_slice(&bytes).map_err(|error| {
            recovery_error("unknown", format!("delete receipt is corrupt: {error}"))
        })
    }

    fn read_receipt(&self, path: &Path) -> Result<InstallReceipt, PackServiceError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            recovery_error("unknown", format!("cannot inspect receipt: {error}"))
        })?;
        if is_link_or_reparse(&metadata)
            || !metadata.is_file()
            || metadata.len() > MAX_RECEIPT_BYTES
        {
            return Err(recovery_error(
                "unknown",
                "receipt is linked, not a regular file, or exceeds its hard size limit",
            ));
        }
        let bytes = fs::read(path)
            .map_err(|error| recovery_error("unknown", format!("cannot read receipt: {error}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| recovery_error("unknown", format!("receipt is corrupt: {error}")))
    }

    fn remove_receipt(&self, path: &Path) -> Result<(), std::io::Error> {
        let metadata = fs::symlink_metadata(path)?;
        if is_link_or_reparse(&metadata) || !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "receipt is not a plain file",
            ));
        }
        fs::remove_file(path)?;
        sync_parent_directory(path)
    }

    fn audit_pack_directories(
        &self,
        records: &[InstalledPackRecord],
    ) -> Result<(), PackServiceError> {
        let packs_root = self.content_root.join("packs");
        if !packs_root.exists() {
            return Ok(());
        }
        ensure_plain_directory(&packs_root, "packs root")?;
        let expected = records
            .iter()
            .map(|record| PathBuf::from(&record.install_path))
            .collect::<HashSet<_>>();
        for pack_entry in fs::read_dir(&packs_root)
            .map_err(|error| recovery_error("unknown", format!("cannot audit packs: {error}")))?
        {
            let pack_entry = pack_entry.map_err(|error| {
                recovery_error("unknown", format!("cannot audit pack entry: {error}"))
            })?;
            let pack_metadata = fs::symlink_metadata(pack_entry.path()).map_err(|error| {
                recovery_error("unknown", format!("cannot inspect pack entry: {error}"))
            })?;
            let pack_name = pack_entry.file_name().to_string_lossy().into_owned();
            if canonical_pack_path(&pack_name).is_none()
                || is_link_or_reparse(&pack_metadata)
                || !pack_metadata.is_dir()
            {
                return Err(recovery_error(
                    &pack_name,
                    "unsafe entry exists in packs root",
                ));
            }
            let mut found_target = false;
            for target_entry in fs::read_dir(pack_entry.path()).map_err(|error| {
                recovery_error(&pack_name, format!("cannot audit pack targets: {error}"))
            })? {
                found_target = true;
                let target_entry = target_entry.map_err(|error| {
                    recovery_error(&pack_name, format!("cannot audit target entry: {error}"))
                })?;
                let metadata = fs::symlink_metadata(target_entry.path()).map_err(|error| {
                    recovery_error(&pack_name, format!("cannot inspect target: {error}"))
                })?;
                let target_name = target_entry.file_name().to_string_lossy().into_owned();
                if canonical_pack_path(&target_name).is_none()
                    || is_link_or_reparse(&metadata)
                    || !metadata.is_dir()
                    || !expected.contains(&target_entry.path())
                {
                    return Err(recovery_error(
                        &pack_name,
                        "untracked, unsafe, or linked pack target exists; it was not deleted",
                    ));
                }
            }
            if !found_target
                && !expected
                    .iter()
                    .any(|path| path.parent() == Some(&pack_entry.path()))
            {
                return Err(recovery_error(
                    &pack_name,
                    "untracked empty pack directory exists; it was not deleted",
                ));
            }
        }
        Ok(())
    }

    fn validate_record_manifest(
        &mut self,
        record: &InstalledPackRecord,
    ) -> Result<ContentPackManifest, PackServiceError> {
        if record.canonical_manifest.len() as u64 > self.limits.max_manifest_bytes {
            return Err(registry_preflight_error(
                &record.pack_id,
                "stored canonical manifest exceeds its hard size limit",
            ));
        }
        let manifest: ContentPackManifest = serde_json::from_str(&record.canonical_manifest)
            .map_err(|error| {
                registry_preflight_error(
                    &record.pack_id,
                    format!("stored canonical manifest is invalid: {error}"),
                )
            })?;
        if record.status == VALIDATED_STATUS {
            manifest
                .validate_published()
                .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        } else if record.status == OWNER_WAIVED_BUNDLED_STATUS {
            manifest
                .validate_bundled_owner_waived()
                .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
            let trust = TRUST.ok_or_else(|| {
                registry_preflight_error(
                    &record.pack_id,
                    "this build has no private-beta trust metadata",
                )
            })?;
            if record.pack_id == trust.pack_id {
                self.verify_private_beta_trust(&manifest, trust)?;
            } else if !RETIRED_PRIVATE_BETA_PACK_IDS.contains(&record.pack_id.as_str()) {
                return Err(registry_preflight_error(
                    &record.pack_id,
                    "private-beta pack is not trusted by this build",
                ));
            }
            if record.pack_id == trust.pack_id && record.archive_sha256 != trust.bundle_sha256 {
                return Err(registry_preflight_error(
                    &record.pack_id,
                    "private-beta bundle hash differs from build-generated metadata",
                ));
            }
        } else if record.status == GENERATED_LOCAL_STATUS {
            let job_id = record
                .pack_id
                .strip_prefix("generated.local.")
                .ok_or_else(|| {
                    registry_preflight_error(&record.pack_id, "generated-local pack id is invalid")
                })?;
            manifest
                .validate_generated_local(job_id)
                .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
            let evidence = self
                .registry
                .find_generated_local_evidence(&record.pack_id)?
                .ok_or_else(|| {
                    registry_preflight_error(&record.pack_id, "generated-local evidence is missing")
                })?;
            let parsed: catalogue::LocalGenerationEvidence =
                serde_json::from_str(&evidence.evidence_json).map_err(|error| {
                    registry_preflight_error(
                        &record.pack_id,
                        format!("generated-local evidence is invalid: {error}"),
                    )
                })?;
            GeneratedLocalRecord {
                generation_job_id: evidence.generation_job_id,
                manifest: manifest.clone(),
                evidence: parsed,
            }
            .validate()
            .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        } else {
            return Err(registry_preflight_error(
                &record.pack_id,
                "unknown registry status",
            ));
        }
        let pinned_listening_test = record.status == OWNER_WAIVED_BUNDLED_STATUS
            && TRUST.is_some_and(|trust| !trust.published && trust.pack_id == record.pack_id);
        if !pinned_listening_test {
            manifest
                .validate_app_compatibility(env!("CARGO_PKG_VERSION"))
                .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        }
        let expected = self.expected_install_path(&manifest);
        let canonical = catalogue::canonical_manifest_bytes(&manifest)
            .map_err(|error| registry_preflight_error(&record.pack_id, error.to_string()))?;
        if Path::new(&record.install_path) != expected
            || record.pack_id != manifest.pack.id
            || record.title != manifest.pack.title
            || record.version != manifest.pack.version
            || record.item_count as usize != manifest.items.len()
            || !matches!(
                record.status.as_str(),
                VALIDATED_STATUS | OWNER_WAIVED_BUNDLED_STATUS | GENERATED_LOCAL_STATUS
            )
            || canonical != record.canonical_manifest.as_bytes()
            || catalogue::import::hash_bytes(&canonical) != record.manifest_sha256
            || !valid_sha256(&record.archive_sha256)
        {
            return Err(registry_preflight_error(
                &record.pack_id,
                "registry metadata, path, status, or hashes differ from the validated manifest",
            ));
        }
        let verify = if record.status == OWNER_WAIVED_BUNDLED_STATUS {
            verify_bundled_owner_waived_pack(&expected, &record.manifest_sha256)
        } else if record.status == GENERATED_LOCAL_STATUS {
            verify_generated_local_pack(&expected, &record.manifest_sha256)
        } else {
            verify_installed_pack(&expected, &record.manifest_sha256)
        };
        verify.map_err(|error| PackServiceError::CorruptRegistry {
            pack_id: record.pack_id.clone(),
            reason: error.to_string(),
        })?;
        Ok(manifest)
    }

    fn expected_install_path(&self, manifest: &ContentPackManifest) -> PathBuf {
        self.content_root
            .join("packs")
            .join(&manifest.pack.id)
            .join(version_storage_key(&manifest.pack.version))
    }
}

fn verify_trusted_bundled_pack(
    path: &Path,
    trust: PrivateBetaTrust,
) -> Result<ContentPackManifest, catalogue::PackImportError> {
    if trust.published {
        verify_installed_pack(path, trust.manifest_sha256)
    } else {
        verify_bundled_owner_waived_pack(path, trust.manifest_sha256)
    }
}

fn is_retired_private_beta(record: &InstalledPackRecord) -> bool {
    record.status == OWNER_WAIVED_BUNDLED_STATUS
        && TRUST.is_some_and(|trust| record.pack_id != trust.pack_id)
        && RETIRED_PRIVATE_BETA_PACK_IDS.contains(&record.pack_id.as_str())
}

impl<R: CatalogueRegistry + GenrePreferenceStore + MoodPreferenceStore + ItemFeedbackStore>
    PackService<R>
{
    /// Revalidates installed packs before exposing both the saved choice and
    /// currently playable options. A saved, no-longer-playable ID remains
    /// visible so callers can require an explicit recovery choice.
    pub(crate) fn genre_state(
        &mut self,
        activity: Activity,
    ) -> Result<ActivityGenreState, PackServiceError> {
        let records = self.validated_records()?;
        let eligibility = self.private_beta_eligibility(&records);
        let manifests = records
            .into_iter()
            .map(|(_, manifest)| manifest)
            .collect::<Vec<_>>();
        let available_genres =
            available_genres_with_eligibility(&manifests, activity, &eligibility)
                .into_iter()
                .map(|GenreOption { id, label }| GenreOptionDto { id, label })
                .collect::<Vec<_>>();
        let selected_genre_id = self.registry.load_genre_preference(activity)?;
        let selected_genre_available = selected_genre_id
            .as_ref()
            .is_none_or(|selected| available_genres.iter().any(|option| &option.id == selected));
        Ok(ActivityGenreState {
            selected_genre_id,
            available_genres,
            selected_genre_available,
        })
    }

    pub(crate) fn set_genre_preference(
        &mut self,
        activity: Activity,
        genre_id: Option<&str>,
    ) -> Result<ActivityGenreState, PackServiceError> {
        if let Some(genre_id) = genre_id {
            if !is_stable_identifier(genre_id) {
                return Err(PersistenceError::InvalidGenreId(genre_id.to_owned()).into());
            }
            let state = self.genre_state(activity)?;
            if !state
                .available_genres
                .iter()
                .any(|option| option.id == genre_id)
            {
                return Err(PackServiceError::Audio(format!(
                    "genre '{genre_id}' is not currently available for this activity; choose Any compatible genre or an available genre"
                )));
            }
            self.registry.save_genre_preference(activity, genre_id)?;
        } else {
            self.registry.clear_genre_preference(activity)?;
        }
        self.genre_state(activity)
    }

    pub(crate) fn mood_state(
        &mut self,
        activity: Activity,
        genre_id: Option<&str>,
    ) -> Result<ActivityMoodState, PackServiceError> {
        self.install_bundled_private_beta()?;
        let records = self.validated_records()?;
        let manifests = records
            .iter()
            .map(|(_, manifest)| manifest.clone())
            .collect::<Vec<_>>();
        let eligibility = self.private_beta_eligibility(&records);
        let item_ids = manifests
            .iter()
            .flat_map(|manifest| manifest.items.iter().map(|item| item.id.clone()))
            .collect::<Vec<_>>();
        let item_feedback = self.registry.load_item_feedback(activity, &item_ids)?;
        let available_moods = available_moods_with_eligibility(
            &manifests,
            activity,
            genre_id,
            &item_feedback,
            &eligibility,
        )
        .into_iter()
        .map(|MoodOption { id, label }| MoodOptionDto { id, label })
        .collect::<Vec<_>>();
        let selected_mood_id = self.registry.load_mood_preference(activity)?;
        let selected_mood_available = selected_mood_id
            .as_ref()
            .is_none_or(|selected| available_moods.iter().any(|option| &option.id == selected));
        Ok(ActivityMoodState {
            selected_mood_id,
            available_moods,
            selected_mood_available,
        })
    }

    pub(crate) fn set_mood_preference(
        &mut self,
        activity: Activity,
        genre_id: Option<&str>,
        mood_id: Option<&str>,
    ) -> Result<ActivityMoodState, PackServiceError> {
        if let Some(mood_id) = mood_id {
            if !is_stable_identifier(mood_id) {
                return Err(PersistenceError::InvalidMoodId(mood_id.to_owned()).into());
            }
            if !self
                .mood_state(activity, genre_id)?
                .available_moods
                .iter()
                .any(|option| option.id == mood_id)
            {
                return Err(PackServiceError::Persistence(PersistenceError::Storage(format!("mood '{mood_id}' is not currently available for this activity and genre; choose Any compatible mood or an available mood"))));
            }
            self.registry.save_mood_preference(activity, mood_id)?;
        } else {
            self.registry.clear_mood_preference(activity)?;
        }
        self.mood_state(activity, genre_id)
    }
}

fn recovery_error(pack_id: impl Into<String>, reason: impl Into<String>) -> PackServiceError {
    PackServiceError::Recovery {
        pack_id: pack_id.into(),
        reason: reason.into(),
    }
}

fn registry_preflight_error(pack_id: &str, reason: impl Into<String>) -> PackServiceError {
    PackServiceError::CorruptRegistry {
        pack_id: pack_id.to_owned(),
        reason: format!(
            "registry validation failed before installed-tree access: {}",
            reason.into()
        ),
    }
}

fn version_storage_key(version: &str) -> String {
    catalogue::import::hash_bytes(version.as_bytes())
}

fn receipt_file_name(pack_id: &str, version: &str) -> String {
    catalogue::import::hash_bytes(format!("{pack_id}\0{version}").as_bytes())
}

fn delete_receipt_file_name(receipt: &DeleteReceipt) -> String {
    catalogue::import::hash_bytes(
        format!(
            "delete\0{}\0{}\0{}",
            receipt.pack_id, receipt.item_id, receipt.version
        )
        .as_bytes(),
    )
}

fn ensure_plain_directory(path: &Path, label: &str) -> Result<(), PackServiceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| recovery_error("unknown", format!("cannot inspect {label}: {error}")))?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(recovery_error(
            "unknown",
            format!("{label} is linked, a reparse point, or not a directory"),
        ));
    }
    Ok(())
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn cleanup_empty_install_parents(target: &Path, packs_root: &Path) {
    if let Some(pack_directory) = target.parent() {
        let _ = fs::remove_dir(pack_directory);
    }
    let _ = fs::remove_dir(packs_root);
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), std::io::Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), std::io::Error> {
    // Windows requires opening directory handles with FILE_FLAG_BACKUP_SEMANTICS.
    // Durable receipts plus startup reconciliation are the explicit fallback.
    Ok(())
}

fn registration_for(
    manifest: &ContentPackManifest,
    target: &Path,
    staged: &catalogue::StagedPack,
) -> Result<PackRegistration, PackServiceError> {
    let canonical_manifest =
        String::from_utf8(staged.canonical_manifest.clone()).map_err(|error| {
            PackServiceError::CorruptRegistry {
                pack_id: manifest.pack.id.clone(),
                reason: error.to_string(),
            }
        })?;
    Ok(registration_from_manifest(
        manifest,
        target,
        canonical_manifest,
        staged.manifest_sha256.clone(),
        staged.archive_sha256.clone(),
    ))
}

fn registration_from_manifest(
    manifest: &ContentPackManifest,
    target: &Path,
    canonical_manifest: String,
    manifest_sha256: String,
    archive_sha256: String,
) -> PackRegistration {
    registration_from_manifest_with_status(
        manifest,
        target,
        canonical_manifest,
        manifest_sha256,
        archive_sha256,
        VALIDATED_STATUS,
    )
}

fn registration_from_manifest_with_status(
    manifest: &ContentPackManifest,
    target: &Path,
    canonical_manifest: String,
    manifest_sha256: String,
    archive_sha256: String,
    status: &str,
) -> PackRegistration {
    let taxonomy = manifest
        .taxonomy
        .genres
        .iter()
        .map(|term| RegisteredTaxonomyTerm {
            kind: "genre".to_owned(),
            term_id: term.id.clone(),
            label: term.label.clone(),
        })
        .chain(
            manifest
                .taxonomy
                .moods
                .iter()
                .map(|term| RegisteredTaxonomyTerm {
                    kind: "mood".to_owned(),
                    term_id: term.id.clone(),
                    label: term.label.clone(),
                }),
        )
        .collect();
    PackRegistration {
        pack: InstalledPackRecord {
            pack_id: manifest.pack.id.clone(),
            title: manifest.pack.title.clone(),
            version: manifest.pack.version.clone(),
            manifest_sha256,
            archive_sha256,
            install_path: target.to_string_lossy().into_owned(),
            item_count: manifest.items.len() as u32,
            status: status.to_owned(),
            canonical_manifest,
            created_at_unix_seconds: 0,
        },
        items: manifest
            .items
            .iter()
            .map(|item| RegisteredItem {
                item_id: item.id.clone(),
                title: item.title.clone(),
            })
            .collect(),
        taxonomy,
        generated_local_evidence: None,
    }
}

fn copy_private_beta_tree(
    source: &Path,
    destination: &Path,
    manifest: &ContentPackManifest,
) -> Result<(), PackServiceError> {
    fs::copy(
        source.join("manifest.json"),
        destination.join("manifest.json"),
    )
    .map_err(|error| {
        recovery_error(
            &manifest.pack.id,
            format!("cannot copy private-beta manifest: {error}"),
        )
    })?;
    for asset in manifest.declared_assets().into_values() {
        let path = canonical_pack_path(&asset.path).ok_or_else(|| {
            recovery_error(&manifest.pack.id, "private-beta asset path is invalid")
        })?;
        let output = destination.join(path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                recovery_error(
                    &manifest.pack.id,
                    format!("cannot create private-beta asset directory: {error}"),
                )
            })?;
        }
        fs::copy(source.join(&asset.path), output).map_err(|error| {
            recovery_error(
                &manifest.pack.id,
                format!("cannot copy private-beta asset: {error}"),
            )
        })?;
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn summary_for(record: &InstalledPackRecord) -> PackSummary {
    PackSummary {
        id: record.pack_id.clone(),
        title: record.title.clone(),
        version: record.version.clone(),
        item_count: record.item_count,
        status: record.status.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use catalogue::manifest::{SafeRegion, FORMAT_VERSION_2};
    use catalogue::{canonical_manifest_bytes, ContentPackManifest};
    use persistence::{CatalogueRegistry, ItemFeedbackStore};
    use serde_json::json;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;

    use audio_engine::{adapt_program_for_device, ProgramRenderer};

    #[derive(Default)]
    struct RegistryState {
        registrations: Vec<PackRegistration>,
        customers: Vec<GeneratedLocalCustomerRecord>,
        fail_register: bool,
        fail_unregister: bool,
        saved_feedback: Vec<(String, Activity, TrackFeedback)>,
    }

    #[derive(Clone)]
    struct MockRegistry(Arc<Mutex<RegistryState>>);

    impl MockRegistry {
        fn new(fail_register: bool) -> (Self, Arc<Mutex<RegistryState>>) {
            let state = Arc::new(Mutex::new(RegistryState {
                fail_register,
                ..RegistryState::default()
            }));
            (Self(state.clone()), state)
        }
    }

    impl CatalogueRegistry for MockRegistry {
        fn list_installed_packs(&mut self) -> Result<Vec<InstalledPackRecord>, PersistenceError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .registrations
                .iter()
                .map(|entry| entry.pack.clone())
                .collect())
        }

        fn find_installed_pack(
            &mut self,
            pack_id: &str,
        ) -> Result<Option<InstalledPackRecord>, PersistenceError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .registrations
                .iter()
                .find(|entry| entry.pack.pack_id == pack_id)
                .map(|entry| entry.pack.clone()))
        }

        fn find_existing_item_ids(
            &mut self,
            item_ids: &[String],
        ) -> Result<Vec<String>, PersistenceError> {
            let state = self.0.lock().unwrap();
            Ok(item_ids
                .iter()
                .filter(|id| {
                    state
                        .registrations
                        .iter()
                        .any(|entry| entry.items.iter().any(|item| &item.item_id == *id))
                })
                .cloned()
                .collect())
        }

        fn register_pack(
            &mut self,
            registration: &PackRegistration,
        ) -> Result<(), PersistenceError> {
            let mut state = self.0.lock().unwrap();
            if state.fail_register {
                return Err(PersistenceError::Storage("injected failure".to_owned()));
            }
            state.registrations.push(registration.clone());
            Ok(())
        }

        fn replace_owner_waived_pack_preserving_feedback(
            &mut self,
            registration: &PackRegistration,
        ) -> Result<(), PersistenceError> {
            let mut state = self.0.lock().unwrap();
            if state.fail_register {
                return Err(PersistenceError::Storage("injected failure".to_owned()));
            }
            let current = state
                .registrations
                .iter_mut()
                .find(|entry| entry.pack.pack_id == registration.pack.pack_id)
                .ok_or_else(|| PersistenceError::Storage("missing legacy pack".into()))?;
            let mut old_ids = current
                .items
                .iter()
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();
            let mut new_ids = registration
                .items
                .iter()
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();
            old_ids.sort();
            new_ids.sort();
            if current.pack.status != OWNER_WAIVED_BUNDLED_STATUS || old_ids != new_ids {
                return Err(PersistenceError::Storage(
                    "invalid legacy replacement".into(),
                ));
            }
            *current = registration.clone();
            Ok(())
        }

        fn find_generated_local_evidence(
            &mut self,
            pack_id: &str,
        ) -> Result<Option<GeneratedLocalEvidenceRecord>, PersistenceError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .registrations
                .iter()
                .find(|entry| entry.pack.pack_id == pack_id)
                .and_then(|entry| entry.generated_local_evidence.clone()))
        }

        fn register_generated_local_pack(
            &mut self,
            registration: &PackRegistration,
            customer: &GeneratedLocalCustomerRecord,
        ) -> Result<(), PersistenceError> {
            let mut state = self.0.lock().unwrap();
            if state.fail_register {
                return Err(PersistenceError::Storage("injected failure".to_owned()));
            }
            if let Some(index) = state
                .registrations
                .iter()
                .position(|entry| entry.pack.pack_id == registration.pack.pack_id)
            {
                if state.registrations[index] != *registration
                    || state
                        .customers
                        .iter()
                        .find(|entry| entry.pack_id == customer.pack_id)
                        != Some(customer)
                {
                    return Err(PersistenceError::Storage("conflict".into()));
                }
                return Ok(());
            }
            state.registrations.push(registration.clone());
            state.customers.push(customer.clone());
            Ok(())
        }

        fn list_generated_local_customers(
            &mut self,
        ) -> Result<Vec<GeneratedLocalCustomerRecord>, PersistenceError> {
            Ok(self.0.lock().unwrap().customers.clone())
        }

        fn rename_generated_local_customer(
            &mut self,
            item_id: &str,
            title: &str,
        ) -> Result<(), PersistenceError> {
            let mut state = self.0.lock().unwrap();
            let customer = state
                .customers
                .iter_mut()
                .find(|entry| entry.item_id == item_id)
                .ok_or_else(|| PersistenceError::UnknownInstalledItem(item_id.into()))?;
            customer.title = title.into();
            Ok(())
        }

        fn unregister_generated_local(
            &mut self,
            item_id: &str,
        ) -> Result<Option<InstalledPackRecord>, PersistenceError> {
            let mut state = self.0.lock().unwrap();
            if state.fail_unregister {
                return Err(PersistenceError::Storage("injected delete failure".into()));
            }
            let Some(customer_index) = state
                .customers
                .iter()
                .position(|entry| entry.item_id == item_id)
            else {
                return Ok(None);
            };
            let customer = state.customers.remove(customer_index);
            let registration_index = state
                .registrations
                .iter()
                .position(|entry| entry.pack.pack_id == customer.pack_id)
                .ok_or_else(|| PersistenceError::Storage("missing pack".into()))?;
            Ok(Some(state.registrations.remove(registration_index).pack))
        }
    }

    impl ItemFeedbackStore for MockRegistry {
        fn load_item_feedback(
            &mut self,
            _activity: Activity,
            _item_ids: &[String],
        ) -> Result<std::collections::BTreeMap<String, TrackFeedback>, PersistenceError> {
            Ok(Default::default())
        }

        fn save_item_feedback(
            &mut self,
            item_id: &str,
            activity: Activity,
            feedback: TrackFeedback,
        ) -> Result<(), PersistenceError> {
            self.0
                .lock()
                .unwrap()
                .saved_feedback
                .push((item_id.to_owned(), activity, feedback));
            Ok(())
        }

        fn clear_item_feedback(
            &mut self,
            _item_id: &str,
            _activity: Activity,
        ) -> Result<(), PersistenceError> {
            Ok(())
        }

        fn load_item_enjoyment(
            &mut self,
            _activity: Activity,
            _item_ids: &[String],
        ) -> Result<std::collections::BTreeMap<String, TrackEnjoyment>, PersistenceError> {
            Ok(Default::default())
        }

        fn save_item_enjoyment(
            &mut self,
            _item_id: &str,
            _activity: Activity,
            _enjoyment: TrackEnjoyment,
        ) -> Result<(), PersistenceError> {
            Ok(())
        }

        fn clear_item_enjoyment(
            &mut self,
            _item_id: &str,
            _activity: Activity,
        ) -> Result<(), PersistenceError> {
            Ok(())
        }
    }

    fn fixture_manifest(asset: &[u8]) -> ContentPackManifest {
        serde_json::from_value(json!({
            "format":"adhdpack","format_version":1,
            "pack":{"id":"test.pack","title":"Test Pack","description":"Fixture","version":"1.0.0","app_version_requirement":">=0.1.0"},
            "taxonomy":{"genres":[{"id":"ambient","label":"Ambient"}],"moods":[{"id":"steady","label":"Steady"}]},
            "items":[{
                "id":"test-item","title":"Test Item","genre_ids":["ambient"],"mood_ids":["steady"],
                "activity_suitability":[{"activity":"deep_work","suitability":0.8},{"activity":"motivation","suitability":0.6},{"activity":"creativity","suitability":0.7},{"activity":"learning","suitability":0.8},{"activity":"light_work","suitability":0.5}],
                "provenance":{"source":"generated test fixture","licence_id":"CC0-1.0","licence_url":"https://creativecommons.org/publicdomain/zero/1.0/","composer":"Test fixture","generator":null,"contains_lyrics":false,"contains_speech":false},
                "analysis":{"duration_seconds":60.0,"integrated_lufs":-20.0,"true_peak_dbfs":-3.0,"loudness_range_lu":4.0,"spectral_centroid_hz":1000.0,"high_frequency_energy_ratio":0.1,"onset_density_per_second":1.0,"tempo_bpm":80.0,"tempo_confidence":0.9,"tempo_drift_percent":1.0,"section_change_novelty":0.1,"unexplained_silence_seconds":0.0,"clipped_samples":0,"discontinuity_detected":false,"codec_errors_detected":false,"corruption_detected":false,"vocal_speech_likelihood":0.0},
                "variants":[{"id":"base","asset":{"path":"assets/test-item.wav","sha256":catalogue::import::hash_bytes(asset),"bytes":asset.len(),"codec":"wav","sample_rate_hz":48000,"channels":2,"bit_depth":16},"safe_regions":[{"kind":"loop","start_seconds":1.0,"end_seconds":59.0}],"stimulation_available":["off","low","medium","high"]}],
                "human_qa":{"status":"approved","reviews":[{"reviewer_id":"reviewer-a","reviewed_at":"2026-01-01","notes":"fixture review","representative_work_session":true},{"reviewer_id":"reviewer-b","reviewed_at":"2026-01-02","notes":"fixture review","representative_work_session":true}]}
            }]
        })).unwrap()
    }

    fn write_pack(root: &Path) -> (PathBuf, Vec<u8>) {
        let asset = b"generated fixture bytes only".to_vec();
        let manifest = fixture_manifest(&asset).canonicalized();
        let archive_path = write_pack_archive(root, "fixture.adhdpack", &asset, &manifest);
        (archive_path, asset)
    }

    fn write_installed_tree(root: &Path, manifest: &ContentPackManifest, asset: &[u8]) {
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets/test-item.wav"), asset).unwrap();
        fs::write(
            root.join("manifest.json"),
            canonical_manifest_bytes(manifest).unwrap(),
        )
        .unwrap();
    }

    fn generated_local_record(job_id: &str, asset: &[u8]) -> GeneratedLocalRecord {
        let mut value = serde_json::to_value(fixture_manifest(asset)).unwrap();
        value["pack"]["id"] = json!(format!("generated.local.{job_id}"));
        value["pack"]["version"] = json!("1.0.0");
        value["pack"]["app_version_requirement"] = json!("*");
        value["items"][0]["id"] = json!(format!("generated.local.{job_id}.item"));
        value["items"][0]["human_qa"] = json!({"status":"draft","reviews":[]});
        value["items"][0]["variants"] = json!([{
            "id":"generated",
            "asset":{
                "path":format!("assets/generated/{job_id}.flac"),
                "sha256":catalogue::import::hash_bytes(asset),
                "bytes":asset.len(),
                "codec":"flac",
                "sample_rate_hz":48000,
                "channels":2,
                "bit_depth":24
            },
            "safe_regions":[{"kind":"loop","start_seconds":1.0,"end_seconds":59.0}],
            "stimulation_available":["off","low","medium","high"]
        }]);
        GeneratedLocalRecord {
            generation_job_id: job_id.to_owned(),
            manifest: serde_json::from_value(value).unwrap(),
            evidence: catalogue::LocalGenerationEvidence {
                producer: "adhd-music-studio".to_owned(),
                job_id: job_id.to_owned(),
                completed_at_unix_seconds: 1,
            },
        }
    }

    fn generated_customer(record: &GeneratedLocalRecord) -> GeneratedLocalCustomerRecord {
        GeneratedLocalCustomerRecord {
            pack_id: record.manifest.pack.id.clone(),
            item_id: record.manifest.items[0].id.clone(),
            title: record.manifest.pack.title.clone(),
            activity: Activity::DeepWork,
            created_at_unix_seconds: record.evidence.completed_at_unix_seconds,
        }
    }

    fn write_generated_staging(
        directory: &Path,
        record: &GeneratedLocalRecord,
        asset: &[u8],
    ) -> PathBuf {
        let directory = directory.to_path_buf();
        fs::create_dir_all(directory.join("assets/generated")).unwrap();
        fs::write(
            directory.join(format!(
                "assets/generated/{}.flac",
                record.generation_job_id
            )),
            asset,
        )
        .unwrap();
        directory
    }

    fn write_playable_pack(root: &Path) -> PathBuf {
        let asset = include_bytes!(
            "../../../../crates/audio-engine/tests/fixtures/wav_pcm16_mono_44100.wav"
        );
        let mut manifest = serde_json::to_value(fixture_manifest(asset)).unwrap();
        manifest["items"][0]["analysis"]["duration_seconds"] = json!(1.0);
        manifest["items"][0]["variants"][0]["asset"]["sample_rate_hz"] = json!(44_100);
        manifest["items"][0]["variants"][0]["asset"]["channels"] = json!(1);
        manifest["items"][0]["variants"][0]["safe_regions"] = json!([{
            "kind": "loop",
            "start_seconds": 0.1,
            "end_seconds": 0.9
        }]);
        let manifest: ContentPackManifest = serde_json::from_value(manifest).unwrap();
        write_pack_archive(
            root,
            "playable-fixture.adhdpack",
            asset,
            &manifest.canonicalized(),
        )
    }

    fn write_playable_opus_pack(root: &Path) -> PathBuf {
        let asset = include_bytes!(
            "../../../../crates/audio-engine/tests/fixtures/ogg_opus_stereo_48000.opus"
        );
        let mut manifest = fixture_manifest(asset);
        manifest.format_version = FORMAT_VERSION_2;
        manifest.items[0].analysis.duration_seconds = 1.0;
        let variant = &mut manifest.items[0].variants[0];
        variant.asset.path = "assets/test-item.opus".to_owned();
        variant.asset.codec = AssetCodec::OggOpus;
        variant.asset.sample_rate_hz = 48_000;
        variant.asset.channels = 2;
        variant.asset.bit_depth = None;
        variant.safe_regions = vec![SafeRegion {
            kind: SafeRegionKind::Loop,
            start_seconds: 0.1,
            end_seconds: 0.9,
        }];
        write_pack_archive(
            root,
            "playable-opus-fixture.adhdpack",
            asset,
            &manifest.canonicalized(),
        )
    }

    fn write_pack_archive(
        root: &Path,
        archive_name: &str,
        asset: &[u8],
        manifest: &ContentPackManifest,
    ) -> PathBuf {
        manifest.validate_published().unwrap();
        let archive_path = root.join(archive_name);
        let file = File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(&canonical_manifest_bytes(manifest).unwrap())
            .unwrap();
        zip.start_file(&manifest.items[0].variants[0].asset.path, options)
            .unwrap();
        zip.write_all(asset).unwrap();
        zip.finish().unwrap();
        archive_path
    }

    fn prepare_crash_after_rename(
        service: &PackService<MockRegistry>,
        archive: &Path,
    ) -> (PathBuf, PathBuf, PackRegistration) {
        let staged = stage_pack(
            archive,
            &service.content_root.join("staging"),
            ImportLimits::default(),
        )
        .unwrap();
        let target = service.expected_install_path(&staged.manifest);
        let registration = registration_for(&staged.manifest, &target, &staged).unwrap();
        let receipt = InstallReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            registration: registration.clone(),
            customer: None,
        };
        let receipt_path = service.write_receipt(&receipt).unwrap();
        staged.install_to(&target).unwrap();
        (receipt_path, target, registration)
    }

    #[test]
    fn import_registers_after_files_and_rejects_idempotent_overwrite() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let content = temp.path().join("content");
        let mut service =
            PackService::with_limits(registry, content.clone(), ImportLimits::default());

        let summary = service.import(&archive).unwrap();
        assert_eq!(summary.id, "test.pack");
        assert_eq!(summary.item_count, 1);
        assert!(content
            .join("packs/test.pack")
            .join(version_storage_key("1.0.0"))
            .join("manifest.json")
            .is_file());
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert!(matches!(
            service.import(&archive),
            Err(PackServiceError::AlreadyInstalled(id)) if id == "test.pack"
        ));
        assert_eq!(service.list().unwrap(), vec![summary]);
    }

    #[test]
    fn generated_local_install_is_job_deterministic_idempotent_and_separate_from_other_trust_statuses(
    ) {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let asset = b"local generated flac fixture";
        let record = generated_local_record("job-local-001", asset);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        let staging = write_generated_staging(&staging, &record, asset);

        let first = service
            .install_generated_local(record.clone(), generated_customer(&record), &staging)
            .unwrap();
        assert_eq!(first.id, "generated.local.job-local-001");
        assert_eq!(first.status, GENERATED_LOCAL_STATUS);
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert_eq!(
            state.lock().unwrap().registrations[0]
                .generated_local_evidence
                .as_ref()
                .unwrap()
                .generation_job_id,
            "job-local-001"
        );
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        let staging = write_generated_staging(&staging, &record, asset);
        assert_eq!(
            service
                .install_generated_local(record.clone(), generated_customer(&record), &staging)
                .unwrap(),
            first
        );
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert_eq!(service.list().unwrap(), vec![first]);
    }

    #[test]
    fn generated_local_install_rejects_sources_outside_controlled_staging() {
        let temp = TempDir::new().unwrap();
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let asset = b"local generated flac fixture";
        let record = generated_local_record("job-local-002", asset);
        let outside = temp.path().join("outside-staging");
        fs::create_dir_all(&outside).unwrap();

        assert!(matches!(
            service.install_generated_local(record.clone(), generated_customer(&record), &outside),
            Err(PackServiceError::Recovery { reason, .. })
                if reason.contains("controlled per-job staging directory")
        ));
    }

    #[test]
    fn generated_local_registry_metadata_tampering_is_rejected() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let asset = b"local generated flac fixture";
        let record = generated_local_record("job-local-003", asset);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        let staging = write_generated_staging(&staging, &record, asset);
        service
            .install_generated_local(record.clone(), generated_customer(&record), &staging)
            .unwrap();
        state.lock().unwrap().registrations[0]
            .generated_local_evidence
            .as_mut()
            .unwrap()
            .generation_job_id = "other-job".to_owned();

        assert!(matches!(
            service.list(),
            Err(PackServiceError::CorruptRegistry { reason, .. })
                if reason.contains("generated-local")
        ));
    }

    #[test]
    fn my_music_delete_is_exact_idempotent_and_removes_registry_and_files() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-delete-001", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record, customer.clone(), &staging)
            .unwrap();
        let target = PathBuf::from(&state.lock().unwrap().registrations[0].pack.install_path);

        assert!(service.delete_my_music(&customer.item_id).unwrap());
        assert!(!target.exists());
        assert!(state.lock().unwrap().registrations.is_empty());
        assert!(state.lock().unwrap().customers.is_empty());
        assert!(!service.delete_my_music(&customer.item_id).unwrap());
    }

    #[test]
    fn my_music_delete_database_failure_restores_files_and_registry() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-delete-002", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record, customer.clone(), &staging)
            .unwrap();
        let target = PathBuf::from(&state.lock().unwrap().registrations[0].pack.install_path);
        state.lock().unwrap().fail_unregister = true;

        assert!(service.delete_my_music(&customer.item_id).is_err());
        assert!(target.is_dir());
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert_eq!(state.lock().unwrap().customers.len(), 1);
    }

    #[test]
    fn generated_local_registration_failure_is_reconciled_from_receipt() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(true);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-save-recover", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");

        assert!(matches!(
            service.install_generated_local(record, customer.clone(), &staging),
            Err(PackServiceError::Recovery { .. })
        ));
        assert_eq!(
            fs::read_dir(service.content_root.join("receipts"))
                .unwrap()
                .count(),
            1
        );
        state.lock().unwrap().fail_register = false;

        assert_eq!(service.list_my_music().unwrap().len(), 1);
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert_eq!(state.lock().unwrap().customers, vec![customer]);
        assert_eq!(
            fs::read_dir(service.content_root.join("receipts"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn generated_local_retry_rejects_conflicting_customer_metadata() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-save-conflict", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record.clone(), customer.clone(), &staging)
            .unwrap();
        state.lock().unwrap().customers[0].title = "Conflicting title".into();
        write_generated_staging(&staging, &record, b"generated fixture");

        assert!(service
            .install_generated_local(record, customer, &staging)
            .is_err());
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
    }

    #[test]
    fn interrupted_uncommitted_delete_is_restored_from_receipt() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-delete-restore", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record, customer, &staging)
            .unwrap();
        let installed = state.lock().unwrap().registrations[0].pack.clone();
        let receipt = DeleteReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            pack_id: installed.pack_id.clone(),
            item_id: format!("{}.item", installed.pack_id),
            version: installed.version,
        };
        let receipt_path = service.write_delete_receipt(&receipt).unwrap();
        let trash = service.content_root.join("delete-trash");
        fs::create_dir_all(&trash).unwrap();
        let tombstone = trash.join(delete_receipt_file_name(&receipt));
        fs::rename(&installed.install_path, &tombstone).unwrap();

        assert_eq!(service.list_my_music().unwrap().len(), 1);
        assert!(Path::new(&installed.install_path).is_dir());
        assert!(!tombstone.exists());
        assert!(!receipt_path.exists());
    }

    #[test]
    fn committed_my_music_delete_is_finished_from_its_durable_receipt() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-delete-003", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record, customer.clone(), &staging)
            .unwrap();
        let installed = state.lock().unwrap().registrations[0].pack.clone();
        let receipt = DeleteReceipt {
            format_version: RECEIPT_FORMAT_VERSION,
            pack_id: installed.pack_id.clone(),
            item_id: customer.item_id.clone(),
            version: installed.version,
        };
        let receipt_path = service.write_delete_receipt(&receipt).unwrap();
        let trash = service.content_root.join("delete-trash");
        fs::create_dir_all(&trash).unwrap();
        let tombstone = trash.join(delete_receipt_file_name(&receipt));
        fs::rename(&installed.install_path, &tombstone).unwrap();
        service
            .registry
            .unregister_generated_local(&customer.item_id)
            .unwrap();

        assert!(service.list_my_music().unwrap().is_empty());
        assert!(!receipt_path.exists());
        assert!(!tombstone.exists());
    }

    #[test]
    fn my_music_delete_rejects_tampered_path_before_touching_outside_files() {
        let temp = TempDir::new().unwrap();
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let record = generated_local_record("job-delete-004", b"generated fixture");
        let customer = generated_customer(&record);
        let staging = service
            .content_root
            .join("studio-staging")
            .join(&record.generation_job_id);
        write_generated_staging(&staging, &record, b"generated fixture");
        service
            .install_generated_local(record, customer.clone(), &staging)
            .unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        state.lock().unwrap().registrations[0].pack.install_path = service
            .content_root
            .join("packs")
            .join("..")
            .join("..")
            .join("outside")
            .to_string_lossy()
            .into_owned();

        assert!(service.delete_my_music(&customer.item_id).is_err());
        assert!(outside.join("keep.txt").is_file());
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
    }

    #[test]
    fn eligible_but_undecodable_installed_asset_is_a_visible_start_failure() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        service.import(&archive).unwrap();

        assert!(matches!(
            service.prepare_playback(Activity::DeepWork, None, None),
            Err(PackServiceError::Audio(_))
        ));
        assert!(service.recent_item_id.is_none());
    }

    #[test]
    fn feedback_uses_the_requested_validated_item_and_never_redirects_to_a_new_source() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        service.import(&archive).unwrap();
        let changed_source = SourceLabel {
            pack_id: "test.pack".to_owned(),
            pack_title: "Test Pack".to_owned(),
            item_id: "new-source".to_owned(),
            item_title: "New source".to_owned(),
            variant_id: "base".to_owned(),
        };

        assert!(matches!(
            service.save_feedback_for_displayed_item(
                Activity::DeepWork,
                "test-item",
                Some(TrackFeedback::HelpsFocus),
                None,
                &changed_source,
            ),
            Err(PackServiceError::Audio(_))
        ));
        assert!(state.lock().unwrap().saved_feedback.is_empty());

        service.commit_playback("test-item".to_owned());
        service
            .save_feedback_for_displayed_item(
                Activity::DeepWork,
                "test-item",
                Some(TrackFeedback::HelpsFocus),
                None,
                &changed_source,
            )
            .unwrap();
        assert_eq!(
            state.lock().unwrap().saved_feedback,
            vec![(
                "test-item".to_owned(),
                Activity::DeepWork,
                TrackFeedback::HelpsFocus,
            )]
        );

        assert!(matches!(
            service.feedback_state_for_displayed_item(
                Activity::DeepWork,
                "removed-item",
                &changed_source,
            ),
            Err(PackServiceError::Audio(_))
        ));
    }

    #[test]
    fn valid_imported_audio_fixture_prepares_deep_work_playback() {
        let temp = TempDir::new().unwrap();
        let archive = write_playable_pack(temp.path());
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));

        let summary = service.import(&archive).unwrap();
        assert_eq!(summary.id, "test.pack");

        let prepared = service
            .prepare_playback(Activity::DeepWork, None, None)
            .unwrap()
            .expect("the valid deep-work item should be selected");
        assert_eq!(prepared.primary_item_id, "test-item");
        assert_eq!(prepared.program.tracks.len(), 1);
        let track = &prepared.program.tracks[0];
        assert_eq!(track.sample_rate_hz, 44_100);
        assert_eq!(track.channels, 1);
        assert_eq!(track.samples.len(), 44_100);
        assert_eq!(track.label.item_id, "test-item");
        assert_eq!(track.regions.len(), 1);
        let region = &track.regions[0];
        assert_eq!(region.kind, AuthoredRegionKind::Loop);
        assert!((region.start_seconds - 0.1).abs() < f32::EPSILON);
        assert!((region.end_seconds - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn valid_v2_ogg_opus_fixture_imports_and_prepares_loop_safe_playback() {
        let temp = TempDir::new().unwrap();
        let archive = write_playable_opus_pack(temp.path());
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));

        service.import(&archive).unwrap();
        let prepared = service
            .prepare_playback(Activity::DeepWork, None, None)
            .unwrap()
            .expect("the valid Ogg Opus item should be selected");
        let track = &prepared.program.tracks[0];
        assert_eq!(track.sample_rate_hz, 48_000);
        assert_eq!(track.channels, 2);
        assert_eq!(track.samples.len(), 96_000);
        assert_eq!(track.regions.len(), 1);
        assert_eq!(track.regions[0].kind, AuthoredRegionKind::Loop);
        assert!((track.regions[0].start_seconds - 0.1).abs() < f32::EPSILON);
        assert!((track.regions[0].end_seconds - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn owner_waived_bundle_upgrades_owner_waived_tree_and_registry_exactly() {
        let temp = TempDir::new().unwrap();
        let content_root = temp.path().join("content");
        let resource = temp.path().join("resource");
        let asset = b"bundled-upgrade-fixture";
        let mut published = fixture_manifest(asset);
        published.pack.version = "2.0.0".into();
        published.items[0].human_qa.status = catalogue::manifest::HumanQaStatus::Draft;
        published.items[0].human_qa.reviews.clear();
        published = published.canonicalized();
        let mut legacy = published.clone();
        legacy.pack.version = "1.0.0".into();
        legacy.items[0].human_qa.status = catalogue::manifest::HumanQaStatus::Draft;
        legacy.items[0].human_qa.reviews.clear();
        legacy = legacy.canonicalized();
        let legacy_path = content_root
            .join("packs")
            .join(&legacy.pack.id)
            .join(version_storage_key(&legacy.pack.version));
        write_installed_tree(&legacy_path, &legacy, asset);
        write_installed_tree(&resource, &published, asset);
        let legacy_bytes = canonical_manifest_bytes(&legacy).unwrap();
        let published_bytes = canonical_manifest_bytes(&published).unwrap();
        let published_hash = catalogue::import::hash_bytes(&published_bytes);
        let bundle_hash = "b".repeat(64);
        let legacy_registration = registration_from_manifest_with_status(
            &legacy,
            &legacy_path,
            String::from_utf8(legacy_bytes.clone()).unwrap(),
            catalogue::import::hash_bytes(&legacy_bytes),
            "a".repeat(64),
            OWNER_WAIVED_BUNDLED_STATUS,
        );
        let (registry, state) = MockRegistry::new(false);
        state
            .lock()
            .unwrap()
            .registrations
            .push(legacy_registration);
        let mut service = PackService::new(registry, content_root);
        let target = service.expected_install_path(&published);
        let replacement = registration_from_manifest_with_status(
            &published,
            &target,
            String::from_utf8(published_bytes).unwrap(),
            published_hash.clone(),
            bundle_hash.clone(),
            OWNER_WAIVED_BUNDLED_STATUS,
        );
        let trust = PrivateBetaTrust {
            pack_id: Box::leak(published.pack.id.clone().into_boxed_str()),
            version: Box::leak(published.pack.version.clone().into_boxed_str()),
            manifest_sha256: Box::leak(published_hash.into_boxed_str()),
            bundle_sha256: Box::leak(bundle_hash.into_boxed_str()),
            item_ids: &["test-item"],
            published: false,
        };

        let legacy_record = state.lock().unwrap().registrations[0].pack.clone();
        service
            .upgrade_owner_waived_bundled_library(
                &legacy_record,
                &published,
                &resource,
                &target,
                &replacement,
                trust,
            )
            .unwrap();

        assert!(!legacy_path.exists());
        assert!(target.exists());
        assert_eq!(state.lock().unwrap().registrations[0], replacement);
    }

    #[test]
    fn retired_owner_waived_pack_with_historical_app_range_does_not_block_startup() {
        let Some(trust) = TRUST else {
            // Retirement is meaningful only in a build that pins a successor.
            return;
        };
        let retired_id = RETIRED_PRIVATE_BETA_PACK_IDS[0];
        assert_ne!(trust.pack_id, retired_id);

        let temp = TempDir::new().unwrap();
        let content_root = temp.path().join("content");
        let asset = b"retired-owner-waived-fixture";
        let mut retired = fixture_manifest(asset);
        retired.pack.id = retired_id.into();
        retired.pack.version = "0.1.0-test.1".into();
        retired.pack.app_version_requirement = ">=0.1.0, <0.2.0".into();
        retired.items[0].human_qa.status = catalogue::manifest::HumanQaStatus::Draft;
        retired.items[0].human_qa.reviews.clear();
        retired = retired.canonicalized();

        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, content_root);
        let retired_path = service.expected_install_path(&retired);
        write_installed_tree(&retired_path, &retired, asset);
        let canonical = canonical_manifest_bytes(&retired).unwrap();
        let registration = registration_from_manifest_with_status(
            &retired,
            &retired_path,
            String::from_utf8(canonical.clone()).unwrap(),
            catalogue::import::hash_bytes(&canonical),
            "a".repeat(64),
            OWNER_WAIVED_BUNDLED_STATUS,
        );
        state.lock().unwrap().registrations.push(registration);

        assert!(service.validated_records().unwrap().is_empty());
        assert!(retired_path.exists());
    }

    #[test]
    fn compiled_private_beta_resource_installs_idempotently_and_prepares_audio() {
        let Some(trust) = TRUST else {
            // Regular builds intentionally compile without private-beta trust.
            return;
        };
        if !trust.published {
            // The real listening library is large. Verify its closed-world
            // manifest and every asset in place instead of copying gigabytes
            // into a test directory and decoding several full tracks.
            let resource = std::env::var_os("ARIA_FOCUS_BUNDLED_PACK_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("private-beta-pack")
                });
            let manifest =
                verify_bundled_owner_waived_pack(&resource, trust.manifest_sha256).unwrap();
            assert_eq!(manifest.pack.id, trust.pack_id);
            assert_eq!(manifest.items.len(), trust.item_ids.len());
            return;
        }
        let temp = TempDir::new().unwrap();
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"))
            .with_resource_dir(Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))));

        let first = service.list().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id, trust.pack_id);
        assert_eq!(
            first[0].status,
            if trust.published {
                VALIDATED_STATUS
            } else {
                OWNER_WAIVED_BUNDLED_STATUS
            }
        );
        assert_eq!(service.list().unwrap(), first);

        let prepared = service
            .prepare_playback(Activity::DeepWork, None, None)
            .unwrap()
            .expect("the compiled private-beta track should be eligible");
        assert!(trust.item_ids.contains(&prepared.primary_item_id.as_str()));

        // The bounded loop queue prepares a multi-track program (the primary
        // plus ranked loop-safe siblings) within the 1..=MAX_PROGRAM_TRACKS
        // bound. The exact count depends on the ranked, sample-budgeted queue,
        // so assert the contract rather than a brittle hardcoded size.
        let track_count = prepared.program.tracks.len();
        assert!(
            (2..=audio_engine::media::MAX_PROGRAM_TRACKS).contains(&track_count),
            "prepared program must be a bounded multi-track queue, got {track_count}",
        );

        // Primary/source integrity: the primary item is the first decoded
        // track and every decoded track is a private-beta-eligible deep-work
        // source with the authored loop region the queue selection required.
        let primary = &prepared.program.tracks[0];
        assert_eq!(primary.label.item_id, prepared.primary_item_id);
        for track in &prepared.program.tracks {
            assert_eq!(track.sample_rate_hz, 48_000);
            assert_eq!(track.channels, 2);
            assert!(
                trust.item_ids.contains(&track.label.item_id.as_str()),
                "track {} is not a private-beta-eligible item",
                track.label.item_id,
            );
            assert_eq!(track.regions.len(), 1);
            assert_eq!(track.regions[0].kind, AuthoredRegionKind::Loop);
        }

        // Starting that prepared source exposes navigation availability: it
        // adapts to the device format and renders as a multi-track program
        // whose renderer starts on the primary. Manual next/previous requests
        // are exercised in the audio-engine unit tests; the renderer request
        // navigation surface is crate-private and not reachable from here.
        let device_program = adapt_program_for_device(&prepared.program, 48_000, 2).unwrap();
        assert!(device_program.tracks.len() >= 2);
        let renderer = ProgramRenderer::new(device_program).unwrap();
        assert_eq!(renderer.current_track(), 0);
    }

    #[test]
    fn database_failure_removes_installed_files() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(true);
        let content = temp.path().join("content");
        let mut service = PackService::new(registry, content.clone());

        assert!(matches!(
            service.import(&archive),
            Err(PackServiceError::Persistence(_))
        ));
        assert!(!content
            .join("packs/test.pack")
            .join(version_storage_key("1.0.0"))
            .exists());
        assert_eq!(fs::read_dir(content.join("receipts")).unwrap().count(), 0);
        assert!(state.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn startup_listing_surfaces_changed_assets_and_registry_metadata() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let content = temp.path().join("content");
        let mut service = PackService::new(registry, content.clone());
        service.import(&archive).unwrap();
        fs::write(
            content
                .join("packs/test.pack")
                .join(version_storage_key("1.0.0"))
                .join("assets/test-item.wav"),
            b"tampered",
        )
        .unwrap();
        assert!(matches!(
            service.list(),
            Err(PackServiceError::CorruptRegistry { .. })
        ));

        let (_, asset) = write_pack(temp.path());
        fs::write(
            content
                .join("packs/test.pack")
                .join(version_storage_key("1.0.0"))
                .join("assets/test-item.wav"),
            asset,
        )
        .unwrap();
        state.lock().unwrap().registrations[0].pack.title = "Wrong".to_owned();
        assert!(matches!(
            service.list(),
            Err(PackServiceError::CorruptRegistry { .. })
        ));
    }

    #[test]
    fn registry_outside_path_is_rejected_before_any_stored_path_access() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let content = temp.path().join("content");
        let mut service = PackService::new(registry, content.clone());
        service.import(&archive).unwrap();

        let outside = temp.path().join("outside-sentinel-pack");
        stage_pack(
            &archive,
            &content.join("staging-outside"),
            ImportLimits::default(),
        )
        .unwrap()
        .install_to(&outside)
        .unwrap();
        let sentinel = temp.path().join("outside-sentinel-do-not-touch");
        fs::write(&sentinel, b"sentinel").unwrap();
        state.lock().unwrap().registrations[0].pack.install_path =
            outside.to_string_lossy().into_owned();

        let error = service.list().unwrap_err().to_string();
        assert!(error.contains("registry validation failed before installed-tree access"));
        assert_eq!(fs::read(sentinel).unwrap(), b"sentinel");
    }

    #[test]
    fn receipt_without_target_is_preserved_and_reported() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let staged = stage_pack(
            &archive,
            &service.content_root.join("staging"),
            ImportLimits::default(),
        )
        .unwrap();
        let target = service.expected_install_path(&staged.manifest);
        let registration = registration_for(&staged.manifest, &target, &staged).unwrap();
        let receipt_path = service
            .write_receipt(&InstallReceipt {
                format_version: RECEIPT_FORMAT_VERSION,
                registration,
                customer: None,
            })
            .unwrap();

        assert!(matches!(
            service.list(),
            Err(PackServiceError::Recovery { .. })
        ));
        assert!(receipt_path.is_file());
        assert!(!target.exists());
    }

    #[test]
    fn verified_target_without_registry_is_finalized_on_startup() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let (receipt_path, target, _) = prepare_crash_after_rename(&service, &archive);

        let listed = service.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
        assert!(target.is_dir());
        assert!(!receipt_path.exists());
    }

    #[test]
    fn registry_committed_with_stale_receipt_is_reconciled() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        service.import(&archive).unwrap();
        let registration = state.lock().unwrap().registrations[0].clone();
        let receipt_path = service
            .write_receipt(&InstallReceipt {
                format_version: RECEIPT_FORMAT_VERSION,
                registration,
                customer: None,
            })
            .unwrap();

        assert_eq!(service.list().unwrap().len(), 1);
        assert!(!receipt_path.exists());
        assert_eq!(state.lock().unwrap().registrations.len(), 1);
    }

    #[test]
    fn corrupt_recovery_target_is_preserved_and_reported() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let (receipt_path, target, _) = prepare_crash_after_rename(&service, &archive);
        fs::write(target.join("assets/test-item.wav"), b"corrupt").unwrap();

        assert!(matches!(
            service.list(),
            Err(PackServiceError::Recovery { .. })
        ));
        assert!(receipt_path.is_file());
        assert!(target.is_dir());
        assert!(state.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn recovery_database_failure_preserves_verified_target_and_receipt() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, state) = MockRegistry::new(true);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let (receipt_path, target, _) = prepare_crash_after_rename(&service, &archive);

        assert!(matches!(
            service.list(),
            Err(PackServiceError::Recovery { .. })
        ));
        assert!(receipt_path.is_file());
        assert!(target.is_dir());
        assert!(state.lock().unwrap().registrations.is_empty());
    }

    #[test]
    fn untracked_pack_target_without_receipt_is_visible_and_not_deleted() {
        let temp = TempDir::new().unwrap();
        let (archive, _) = write_pack(temp.path());
        let (registry, _) = MockRegistry::new(false);
        let mut service = PackService::new(registry, temp.path().join("content"));
        let staged = stage_pack(
            &archive,
            &service.content_root.join("staging"),
            ImportLimits::default(),
        )
        .unwrap();
        let target = service.expected_install_path(&staged.manifest);
        staged.install_to(&target).unwrap();

        assert!(matches!(
            service.list(),
            Err(PackServiceError::Recovery { .. })
        ));
        assert!(target.is_dir());
    }
}
