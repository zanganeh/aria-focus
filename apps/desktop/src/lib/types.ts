export type Intensity = "off" | "low" | "medium" | "high";

export type Activity = "deep_work" | "motivation" | "creativity" | "learning" | "light_work";

export type SessionStatus = "idle" | "playing" | "paused" | "stopped" | "expired";

export type SessionType =
  | { kind: "infinite" }
  | { kind: "countdown"; seconds: number }
  | {
      kind: "interval";
      work_seconds: number;
      break_seconds: number;
      repeats: number;
    };

export interface SessionSnapshot {
  status: SessionStatus;
  activity: Activity;
  intensity: Intensity;
  kind: SessionType;
  phase: "work" | "break" | null;
  current_round: number | null;
  total_rounds: number | null;
  focus_elapsed_seconds: number;
  current_phase_remaining_seconds: number | null;
  total_remaining_seconds: number | null;
}

export interface Provenance {
  asset_id: string;
  title: string;
  generator: string;
  generator_version: string;
  source: string;
  licence: string;
  contains_voice_or_speech: boolean;
  contains_lyrics: boolean;
  notes: string;
  sample_rate_hz: number;
  channels: number;
  duration_seconds: number;
  loops_seamlessly: boolean;
}

export interface ContentPackSummary {
  id: string;
  title: string;
  version: string;
  item_count: number;
  status: "validated_metadata" | "owner_waived_bundled_private_beta";
}

export interface CurrentSource {
  pack_id: string;
  pack_title: string;
  item_id: string;
  item_title: string;
  variant_id: string;
  fallback: boolean;
  quarantined_review?: boolean;
  navigation_available?: boolean;
}

export interface ReviewCandidate {
  alias: string;
  title: string;
  review_id: string;
  bytes: number;
  codec: string;
  sample_rate_hz: number;
  channels: number;
  duration_seconds: number;
  quarantine_status: string;
}

export type TrackFeedback = "helps_focus" | "neutral" | "distracting";
export type TrackEnjoyment = "liked" | "not_for_me";

export interface ItemFeedbackState {
  item_id: string;
  activity: Activity;
  focus_feedback: TrackFeedback | null;
  enjoyment: TrackEnjoyment | null;
}

export interface FavoriteLibraryItem {
  item_id: string;
  activity: Activity;
  title: string;
  genre: string[];
  moods: string[];
}

export interface GenreOption {
  id: string;
  label: string;
}

export interface ActivityGenreState {
  selected_genre_id: string | null;
  available_genres: GenreOption[];
  selected_genre_available: boolean;
}
export interface MoodOption {
  id: string;
  label: string;
}
export interface ActivityMoodState {
  selected_mood_id: string | null;
  available_moods: MoodOption[];
  selected_mood_available: boolean;
}

export interface StartupHealth {
  core_ready: boolean;
  core_error: string | null;
  packs_ready: boolean;
  packs_error: string | null;
  migration_status?: "not_needed" | "migrated" | "conflict" | "failed";
  migration_error?: string | null;
}

export interface OnboardingPreferences {
  completed: boolean;
  intensity: Exclude<Intensity, "off">;
  genres: string[];
}

export type SessionEndReason = "stopped" | "expired" | "interrupted";
export type SessionFocusOutcome = "helped_focus" | "neutral" | "distracting";
export type SessionSoundEnjoyment = "liked" | "not_for_me";
export interface SessionHistoryRecord {
  id: string;
  activity: Activity;
  intensity: Intensity;
  session_type: SessionType;
  started_at: number;
  ended_at: number | null;
  end_reason: SessionEndReason | null;
  focus_seconds: number | null;
  focus_outcome: SessionFocusOutcome | null;
  sound_enjoyment: SessionSoundEnjoyment | null;
}

export type StudioCapabilityState =
  "checking" | "ready" | "setup_required" | "unsupported" | "needs_attention";

export interface StudioCapability {
  state: StudioCapabilityState;
  detail: string | null;
  required_bytes?: number;
  free_bytes?: number;
}
export interface RuntimeInstall {
  status: "idle" | "installing" | "complete";
  stage: string;
  detail: string;
  downloaded_bytes?: number;
  total_bytes?: number | null;
  required_disk_bytes?: number | null;
  resumable?: boolean;
}

export interface StudioJobSummary {
  id: string;
  status: "Ready" | "Saved" | "In progress" | "Needs attention";
  updated_at_ms: number;
  length_seconds: number;
  stage: string;
  can_preview: boolean;
  can_save: boolean;
  can_discard: boolean;
  safe_message: string | null;
}
export interface CreateStudioMusic {
  activity: Activity;
  sound_style_id: "ambient" | "gentle-piano" | "soft-electronic";
  energy: "low" | "medium" | "high";
  duration_seconds: 90 | 180;
  note: string | null;
  parent_job_id: string | null;
}
export interface MyMusicItem {
  item_id: string;
  title: string;
  duration_seconds: number;
  created_at: number;
  activity: Activity;
  job_id: string;
}
