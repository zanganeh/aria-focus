//! Deterministic selection over manifests that have already passed full pack
//! service revalidation. This module never reads the filesystem or registry.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use domain::{Activity, TrackFeedback};

use crate::manifest::{
    AssetCodec, AudioAsset, ContentPackManifest, HumanQaStatus, SafeRegion, SafeRegionKind,
    StimulationAvailability,
};

pub const PLAYBACK_MAX_ENCODED_BYTES: u64 = 512 * 1024 * 1024;
pub const PLAYBACK_MAX_DECODED_SAMPLES: f64 = (64 * 1024 * 1024) as f64;
const MIN_SAFE_REGION_SECONDS: f32 = 0.001;
const MAX_CROSSFADE_SECONDS: f32 = 8.0;

/// Maximum number of distinct loop-safe tracks decoded into a single playback
/// program. The queue is bounded so previous/next navigation stays useful
/// without decoding every future track, and the cumulative decoded sample
/// budget keeps the program decodable.
pub const PLAYBACK_QUEUE_MAX_TRACKS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackCandidate {
    pub pack_id: String,
    pub pack_title: String,
    pub item_id: String,
    pub item_title: String,
    pub variant_id: String,
    pub suitability: f32,
    pub duration_seconds: f32,
    pub asset: AudioAsset,
    pub safe_regions: Vec<SafeRegion>,
    pub genre_ids: Vec<String>,
    pub mood_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenreOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodOption {
    pub id: String,
    pub label: String,
}

/// Pure, deterministic selection inputs. `genre_id: None` means any eligible
/// genre; `Some` requires an exact item-level taxonomy ID match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackSelectionInput<'a> {
    pub activity: Activity,
    pub genre_id: Option<&'a str>,
    pub mood_id: Option<&'a str>,
    pub previous_item_id: Option<&'a str>,
    /// Only feedback for `activity` belongs here. Missing entries and
    /// `Neutral` intentionally rank the same.
    pub item_feedback: &'a BTreeMap<String, TrackFeedback>,
}

/// Explicit exception for an item trusted by the application build. Archive
/// metadata never participates in this decision.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaybackEligibility {
    owner_waived_bundled_item_ids: BTreeSet<String>,
    app_generated_item_ids: BTreeSet<String>,
}

impl PlaybackEligibility {
    pub fn published_only() -> Self {
        Self::default()
    }

    pub fn with_owner_waived_bundled_item(item_id: String) -> Self {
        Self {
            owner_waived_bundled_item_ids: [item_id].into_iter().collect(),
            app_generated_item_ids: BTreeSet::new(),
        }
    }

    pub fn with_owner_waived_bundled_items(item_ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            owner_waived_bundled_item_ids: item_ids.into_iter().collect(),
            app_generated_item_ids: BTreeSet::new(),
        }
    }

    pub fn allowing_owner_waived_bundled_items(
        mut self,
        item_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        self.owner_waived_bundled_item_ids.extend(item_ids);
        self
    }

    pub fn allowing_app_generated_items(
        mut self,
        item_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        self.app_generated_item_ids.extend(item_ids);
        self
    }

    fn permits_app_generated(&self, item_id: &str) -> bool {
        self.app_generated_item_ids.contains(item_id)
    }

    fn permits(&self, item_id: &str, status: HumanQaStatus) -> bool {
        status == HumanQaStatus::Approved
            || self.owner_waived_bundled_item_ids.contains(item_id)
            || self.app_generated_item_ids.contains(item_id)
    }
}

impl PlaybackCandidate {
    pub fn source_key(&self) -> String {
        format!("{}/{}/{}", self.pack_id, self.item_id, self.variant_id)
    }

    pub fn has_region(&self, kind: SafeRegionKind) -> bool {
        self.safe_regions.iter().any(|region| region.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackSelection {
    pub tracks: Vec<PlaybackCandidate>,
}

impl PlaybackSelection {
    pub fn primary(&self) -> &PlaybackCandidate {
        &self.tracks[0]
    }
}

/// Select one loop-safe item or a pair of crossfade-safe items. Callers must
/// pass only manifests returned by the pack service's current full validation
/// pass. Ties are stable across processes and machines.
pub fn select_playback_plan(
    manifests: &[ContentPackManifest],
    input: PlaybackSelectionInput<'_>,
) -> Option<PlaybackSelection> {
    select_playback_plan_with_eligibility(manifests, input, &PlaybackEligibility::published_only())
}

pub fn select_playback_plan_with_eligibility(
    manifests: &[ContentPackManifest],
    input: PlaybackSelectionInput<'_>,
    eligibility: &PlaybackEligibility,
) -> Option<PlaybackSelection> {
    let mut candidates = eligible_candidates(
        manifests,
        input.activity,
        input.genre_id,
        input.mood_id,
        input.item_feedback,
        eligibility,
    );
    candidates.sort_by(|left, right| stable_order(input.item_feedback, left, right));
    let all_candidates = candidates.clone();
    candidates.retain(|candidate| {
        has_usable_loop(candidate)
            || all_candidates
                .iter()
                .any(|other| other.item_id != candidate.item_id && crossfade_pair(candidate, other))
    });
    if candidates.is_empty() {
        return None;
    }

    let distinct_items = candidates
        .iter()
        .map(|candidate| candidate.item_id.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let primary_index = if distinct_items >= 2 {
        candidates
            .iter()
            .position(|candidate| Some(candidate.item_id.as_str()) != input.previous_item_id)
            .unwrap_or(0)
    } else {
        0
    };
    let primary = candidates.remove(primary_index);

    if primary.has_region(SafeRegionKind::Crossfade) {
        if let Some(next_index) = candidates.iter().position(|candidate| {
            candidate.item_id != primary.item_id && crossfade_pair(&primary, candidate)
        }) {
            let next = candidates.remove(next_index);
            return Some(PlaybackSelection {
                tracks: vec![primary, next],
            });
        }
    }

    if has_usable_loop(&primary) {
        return Some(PlaybackSelection {
            tracks: build_loop_queue(primary, &candidates),
        });
    }

    // A crossfade-only primary without a partner cannot play continuously.
    // Prefer the highest-ranked loop-safe fallback rather than ending audio.
    let fallback_index = candidates.iter().position(has_usable_loop)?;
    let fallback = candidates.remove(fallback_index);
    Some(PlaybackSelection {
        tracks: build_loop_queue(fallback, &candidates),
    })
}

/// Build a bounded in-memory playback queue of up to PLAYBACK_QUEUE_MAX_TRACKS
/// distinct loop-safe tracks, primary first, in deterministic ranked order. The
/// cumulative decoded sample budget keeps the queue decodable within the program
/// sample limit, so navigation never over-decodes future tracks. The primary is
/// always present even when it alone consumes the budget; a single eligible
/// track therefore remains a valid single-track program. An over-budget
/// candidate is skipped rather than terminating the queue, so a later smaller
/// candidate can still fill the remaining decoded-sample budget.
fn build_loop_queue(
    primary: PlaybackCandidate,
    candidates: &[PlaybackCandidate],
) -> Vec<PlaybackCandidate> {
    let mut queue = Vec::with_capacity(PLAYBACK_QUEUE_MAX_TRACKS);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut samples = estimated_samples(&primary);
    seen.insert(primary.item_id.clone());
    queue.push(primary);

    for candidate in candidates {
        if queue.len() >= PLAYBACK_QUEUE_MAX_TRACKS {
            break;
        }
        if !has_usable_loop(candidate) || !seen.insert(candidate.item_id.clone()) {
            continue;
        }
        let candidate_samples = estimated_samples(candidate);
        if samples + candidate_samples > PLAYBACK_MAX_DECODED_SAMPLES {
            continue;
        }
        samples += candidate_samples;
        queue.push(candidate.clone());
    }

    queue
}

/// Select a specific already-validated item without consulting focus feedback.
/// A crossfade partner remains eligible so an explicitly chosen item keeps its
/// authored continuous program rather than being reduced to a single track.
pub fn select_playback_plan_for_item_with_eligibility(
    manifests: &[ContentPackManifest],
    activity: Activity,
    item_id: &str,
    eligibility: &PlaybackEligibility,
) -> Option<PlaybackSelection> {
    let empty_feedback = BTreeMap::new();
    let mut all = eligible_candidates(
        manifests,
        activity,
        None,
        None,
        &empty_feedback,
        eligibility,
    );
    all.sort_by_key(|candidate| candidate.source_key());
    let primary = all
        .iter()
        .find(|candidate| candidate.item_id == item_id)?
        .clone();
    if has_usable_loop(&primary) {
        return Some(PlaybackSelection {
            tracks: vec![primary],
        });
    }
    all.into_iter()
        .find(|candidate| candidate.item_id != item_id && crossfade_pair(&primary, candidate))
        .map(|partner| PlaybackSelection {
            tracks: vec![primary, partner],
        })
}

/// Returns only genres that have at least one fully playable candidate for the
/// requested activity. IDs and labels are sorted deterministically by ID.
pub fn available_genres(manifests: &[ContentPackManifest], activity: Activity) -> Vec<GenreOption> {
    available_genres_with_eligibility(manifests, activity, &PlaybackEligibility::published_only())
}

pub fn available_genres_with_eligibility(
    manifests: &[ContentPackManifest],
    activity: Activity,
    eligibility: &PlaybackEligibility,
) -> Vec<GenreOption> {
    let empty_feedback = BTreeMap::new();
    let mut candidates = eligible_candidates(
        manifests,
        activity,
        None,
        None,
        &empty_feedback,
        eligibility,
    );
    candidates.sort_by(|left, right| stable_order(&empty_feedback, left, right));
    let all = candidates.clone();
    candidates.retain(|candidate| {
        has_usable_loop(candidate)
            || all
                .iter()
                .any(|other| other.item_id != candidate.item_id && crossfade_pair(candidate, other))
    });
    let labels = manifests
        .iter()
        .flat_map(|manifest| manifest.taxonomy.genres.iter())
        .fold(BTreeMap::<String, String>::new(), |mut labels, term| {
            labels
                .entry(term.id.clone())
                .and_modify(|current| {
                    if term.label < *current {
                        *current = term.label.clone();
                    }
                })
                .or_insert_with(|| term.label.clone());
            labels
        });
    let mut ids = candidates
        .into_iter()
        .flat_map(|candidate| candidate.genre_ids)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| {
            labels.get(&id).map(|label| GenreOption {
                id,
                label: label.clone(),
            })
        })
        .collect()
}

fn continuously_playable_candidates(
    mut candidates: Vec<PlaybackCandidate>,
) -> Vec<PlaybackCandidate> {
    let all = candidates.clone();
    candidates.retain(|candidate| {
        has_usable_loop(candidate)
            || all
                .iter()
                .any(|other| other.item_id != candidate.item_id && crossfade_pair(candidate, other))
    });
    candidates
}

/// Returns moods with a fully playable candidate for activity and the optional
/// exact genre filter. Ordering is deterministic by stable ID.
pub fn available_moods(
    manifests: &[ContentPackManifest],
    activity: Activity,
    genre_id: Option<&str>,
) -> Vec<MoodOption> {
    available_moods_with_eligibility(
        manifests,
        activity,
        genre_id,
        &BTreeMap::new(),
        &PlaybackEligibility::published_only(),
    )
}

pub fn available_moods_with_eligibility(
    manifests: &[ContentPackManifest],
    activity: Activity,
    genre_id: Option<&str>,
    item_feedback: &BTreeMap<String, TrackFeedback>,
    eligibility: &PlaybackEligibility,
) -> Vec<MoodOption> {
    let candidates = continuously_playable_candidates(eligible_candidates(
        manifests,
        activity,
        genre_id,
        None,
        item_feedback,
        eligibility,
    ));
    let labels = manifests
        .iter()
        .flat_map(|manifest| manifest.taxonomy.moods.iter())
        .fold(BTreeMap::<String, String>::new(), |mut labels, term| {
            labels
                .entry(term.id.clone())
                .and_modify(|current| {
                    if term.label < *current {
                        *current = term.label.clone();
                    }
                })
                .or_insert_with(|| term.label.clone());
            labels
        });
    let mut ids = candidates
        .into_iter()
        .flat_map(|candidate| candidate.mood_ids)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| {
            labels.get(&id).map(|label| MoodOption {
                id,
                label: label.clone(),
            })
        })
        .collect()
}

fn eligible_candidates(
    manifests: &[ContentPackManifest],
    activity: Activity,
    genre_id: Option<&str>,
    mood_id: Option<&str>,
    item_feedback: &BTreeMap<String, TrackFeedback>,
    eligibility: &PlaybackEligibility,
) -> Vec<PlaybackCandidate> {
    let required_levels = [
        StimulationAvailability::Off,
        StimulationAvailability::Low,
        StimulationAvailability::Medium,
        StimulationAvailability::High,
    ];
    manifests
        .iter()
        .flat_map(|manifest| {
            manifest.items.iter().flat_map(move |item| {
                let suitability = item
                    .activity_suitability
                    .iter()
                    .find(|entry| entry.activity == activity)
                    .map(|entry| entry.suitability)
                    .unwrap_or(0.0);
                item.variants.iter().filter_map(move |variant| {
                    let app_generated = eligibility.permits_app_generated(&item.id);
                    let safe_regions = if app_generated
                        && variant.safe_regions.is_empty()
                        && !item.analysis.discontinuity_detected
                        && item.analysis.duration_seconds.is_finite()
                        && item.analysis.duration_seconds > 0.0
                    {
                        vec![SafeRegion {
                            kind: SafeRegionKind::Loop,
                            start_seconds: 0.0,
                            end_seconds: item.analysis.duration_seconds,
                        }]
                    } else {
                        variant.safe_regions.clone()
                    };
                    let safe = safe_regions.iter().any(|region| {
                        matches!(
                            region.kind,
                            SafeRegionKind::Loop | SafeRegionKind::Crossfade
                        )
                    });
                    let all_intensities = required_levels
                        .iter()
                        .all(|level| variant.stimulation_available.contains(level))
                        || (app_generated
                            && variant.stimulation_available == [StimulationAvailability::Off]);
                    let codec_playable = matches!(
                        variant.asset.codec,
                        AssetCodec::Wav | AssetCodec::Flac | AssetCodec::Mp3 | AssetCodec::OggOpus
                    ) && matches!(variant.asset.channels, 1 | 2)
                        && variant.asset.bytes <= PLAYBACK_MAX_ENCODED_BYTES
                        && f64::from(item.analysis.duration_seconds)
                            * f64::from(variant.asset.sample_rate_hz)
                            * f64::from(variant.asset.channels)
                            <= PLAYBACK_MAX_DECODED_SAMPLES;
                    if suitability <= 0.0
                        || item_feedback.get(&item.id) == Some(&TrackFeedback::Distracting)
                        || genre_id
                            .is_some_and(|genre_id| !item.genre_ids.iter().any(|id| id == genre_id))
                        || mood_id
                            .is_some_and(|mood_id| !item.mood_ids.iter().any(|id| id == mood_id))
                        || !eligibility.permits(&item.id, item.human_qa.status)
                        || !safe
                        || !all_intensities
                        || !codec_playable
                    {
                        return None;
                    }
                    Some(PlaybackCandidate {
                        pack_id: manifest.pack.id.clone(),
                        pack_title: manifest.pack.title.clone(),
                        item_id: item.id.clone(),
                        item_title: item.title.clone(),
                        variant_id: variant.id.clone(),
                        suitability,
                        duration_seconds: item.analysis.duration_seconds,
                        asset: variant.asset.clone(),
                        safe_regions,
                        genre_ids: item.genre_ids.clone(),
                        mood_ids: item.mood_ids.clone(),
                    })
                })
            })
        })
        .collect()
}

fn has_usable_loop(candidate: &PlaybackCandidate) -> bool {
    candidate.safe_regions.iter().any(|region| {
        region.kind == SafeRegionKind::Loop
            && region.end_seconds - region.start_seconds >= MIN_SAFE_REGION_SECONDS
    })
}

fn crossfade_pair(left: &PlaybackCandidate, right: &PlaybackCandidate) -> bool {
    if estimated_samples(left) + estimated_samples(right) > PLAYBACK_MAX_DECODED_SAMPLES {
        return false;
    }
    let left_regions = left
        .safe_regions
        .iter()
        .filter(|region| region.kind == SafeRegionKind::Crossfade)
        .collect::<Vec<_>>();
    let right_regions = right
        .safe_regions
        .iter()
        .filter(|region| region.kind == SafeRegionKind::Crossfade)
        .collect::<Vec<_>>();
    left_regions.iter().any(|left_incoming| {
        left_regions.iter().any(|left_outgoing| {
            right_regions.iter().any(|right_incoming| {
                right_regions.iter().any(|right_outgoing| {
                    let left_to_right = (left_outgoing.end_seconds - left_outgoing.start_seconds)
                        .min(right_incoming.end_seconds - right_incoming.start_seconds)
                        .min(MAX_CROSSFADE_SECONDS);
                    let right_to_left = (right_outgoing.end_seconds - right_outgoing.start_seconds)
                        .min(left_incoming.end_seconds - left_incoming.start_seconds)
                        .min(MAX_CROSSFADE_SECONDS);
                    left_to_right >= MIN_SAFE_REGION_SECONDS
                        && right_to_left >= MIN_SAFE_REGION_SECONDS
                        && right_incoming.start_seconds + left_to_right
                            <= right_outgoing.start_seconds
                        && left_incoming.start_seconds + right_to_left
                            <= left_outgoing.start_seconds
                })
            })
        })
    })
}

fn estimated_samples(candidate: &PlaybackCandidate) -> f64 {
    f64::from(candidate.duration_seconds)
        * f64::from(candidate.asset.sample_rate_hz)
        * f64::from(candidate.asset.channels)
}

fn stable_order(
    item_feedback: &BTreeMap<String, TrackFeedback>,
    left: &PlaybackCandidate,
    right: &PlaybackCandidate,
) -> Ordering {
    let feedback_rank = |candidate: &PlaybackCandidate| match item_feedback.get(&candidate.item_id)
    {
        Some(TrackFeedback::HelpsFocus) => 0,
        Some(TrackFeedback::Neutral) | None => 1,
        Some(TrackFeedback::Distracting) => 2,
    };
    feedback_rank(left)
        .cmp(&feedback_rank(right))
        .then_with(|| right.suitability.total_cmp(&left.suitability))
        .then_with(|| left.pack_id.cmp(&right.pack_id))
        .then_with(|| left.item_id.cmp(&right.item_id))
        .then_with(|| left.variant_id.cmp(&right.variant_id))
}
