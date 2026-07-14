import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Activity,
  ContentPackSummary,
  CurrentSource,
  ActivityGenreState,
  ActivityMoodState,
  ItemFeedbackState,
  Intensity,
  Provenance,
  SessionSnapshot,
  SessionType,
  StartupHealth,
  TrackFeedback,
  TrackEnjoyment,
  OnboardingPreferences,
  ReviewCandidate,
  FavoriteLibraryItem,
  SessionHistoryRecord,
  SessionFocusOutcome,
  SessionSoundEnjoyment,
  StudioCapability,
  RuntimeInstall,
  StudioJobSummary,
  CreateStudioMusic,
  MyMusicItem,
} from "./types";

export async function getStudioCapability(): Promise<StudioCapability> {
  return await invoke<StudioCapability>("get_studio_capability");
}
export async function getRuntimeInstall(): Promise<RuntimeInstall> {
  return await invoke<RuntimeInstall>("get_runtime_install");
}
export async function startRuntimeInstall(): Promise<RuntimeInstall> {
  return await invoke<RuntimeInstall>("start_runtime_install");
}
export async function cancelRuntimeInstall(): Promise<RuntimeInstall> {
  return await invoke<RuntimeInstall>("cancel_runtime_install");
}
export async function repairRuntime(): Promise<RuntimeInstall> {
  return await invoke<RuntimeInstall>("repair_runtime");
}
export async function listRecentStudioJobs(): Promise<StudioJobSummary[]> {
  return await invoke<StudioJobSummary[]>("list_recent_studio_jobs");
}
export async function getStudioJob(id: string): Promise<StudioJobSummary | null> {
  return await invoke<StudioJobSummary | null>("get_studio_job", { jobId: id });
}
export async function createStudioMusic(request: CreateStudioMusic): Promise<StudioJobSummary> {
  return await invoke<StudioJobSummary>("create_studio_music", { request });
}
export async function cancelStudioMusic(jobId: string): Promise<StudioJobSummary> {
  return await invoke<StudioJobSummary>("cancel_studio_music", { jobId });
}
export async function regenerateStudioMusic(
  jobId: string,
  request: Omit<CreateStudioMusic, "parent_job_id">,
): Promise<StudioJobSummary> {
  return await createStudioMusic({ ...request, parent_job_id: jobId });
}
export async function startDraftPreview(jobId: string): Promise<void> {
  await invoke("start_draft_preview", { jobId });
}
export async function pauseDraftPreview(): Promise<void> {
  await invoke("pause_draft_preview");
}
export async function resumeDraftPreview(): Promise<void> {
  await invoke("resume_draft_preview");
}
export async function stopDraftPreview(): Promise<void> {
  await invoke("stop_draft_preview");
}
export async function getDraftPreviewState(): Promise<"stopped" | "playing" | "paused"> {
  return await invoke("get_draft_preview_state");
}
export async function saveStudioDraft(jobId: string, title: string): Promise<MyMusicItem> {
  return await invoke("save_studio_draft", { jobId, title });
}
export async function discardStudioDraft(jobId: string): Promise<void> {
  await invoke("discard_studio_draft", { jobId });
}
export async function listMyMusic(): Promise<MyMusicItem[]> {
  return await invoke("list_my_music");
}
export async function renameMyMusic(itemId: string, title: string): Promise<void> {
  await invoke("rename_my_music", { itemId, title });
}
export async function deleteMyMusic(itemId: string): Promise<void> {
  await invoke("delete_my_music", { itemId });
}
export async function startMyMusic(itemId: string, activity: Activity): Promise<void> {
  await invoke("start_my_music", { itemId, activity });
}

export async function getOnboardingPreferences(): Promise<OnboardingPreferences> {
  return await invoke<OnboardingPreferences>("get_onboarding_preferences");
}

export async function completeOnboarding(
  intensity: Exclude<Intensity, "off">,
  genres: string[],
): Promise<void> {
  await invoke("complete_onboarding", { intensity, genres });
}

export async function getStartupHealth(): Promise<StartupHealth> {
  return await invoke<StartupHealth>("get_startup_health");
}
export async function listReviewCandidates(): Promise<ReviewCandidate[]> {
  return await invoke<ReviewCandidate[]>("list_review_candidates");
}
export async function startReviewCandidate(candidateId: string): Promise<void> {
  await invoke("start_review_candidate", { candidateId });
}

export async function retryStartup(): Promise<StartupHealth> {
  return await invoke<StartupHealth>("retry_startup");
}

export async function startSession(): Promise<void> {
  await invoke("start_session");
}
export async function listFavorites(): Promise<FavoriteLibraryItem[]> {
  return await invoke<FavoriteLibraryItem[]>("list_favorites");
}
export async function removeFavorite(itemId: string, activity: Activity): Promise<void> {
  await invoke("remove_favorite", { itemId, activity });
}
export async function startFavorite(itemId: string, activity: Activity): Promise<void> {
  await invoke("start_favorite", { itemId, activity });
}

export async function getActivityGenres(): Promise<ActivityGenreState> {
  return await invoke<ActivityGenreState>("get_activity_genres");
}

export async function setActivityGenre(genreId: string | null): Promise<ActivityGenreState> {
  return await invoke<ActivityGenreState>("set_activity_genre", { genreId });
}
export async function getActivityMoods(): Promise<ActivityMoodState> {
  return await invoke<ActivityMoodState>("get_activity_moods");
}
export async function setActivityMood(moodId: string | null): Promise<ActivityMoodState> {
  return await invoke<ActivityMoodState>("set_activity_mood", { moodId });
}

export async function pauseSession(): Promise<void> {
  await invoke("pause_session");
}

export async function resumeSession(): Promise<void> {
  await invoke("resume_session");
}

export async function stopSession(): Promise<void> {
  await invoke("stop_session");
}
export async function listRecentSessions(): Promise<SessionHistoryRecord[]> {
  return await invoke<SessionHistoryRecord[]>("list_recent_sessions");
}
export async function saveSessionRating(
  id: string,
  focusOutcome: SessionFocusOutcome | null,
  soundEnjoyment: SessionSoundEnjoyment | null,
): Promise<void> {
  await invoke("save_session_rating", { id, focusOutcome, soundEnjoyment });
}
export async function nextTrack(): Promise<void> {
  await invoke("next_track");
}
export async function previousTrack(): Promise<void> {
  await invoke("previous_track");
}

export async function setActivity(activity: Activity): Promise<void> {
  await invoke("set_activity", { activity });
}

export async function setIntensity(intensity: Intensity): Promise<void> {
  await invoke("set_intensity", { intensity });
}

export async function getMasterVolume(): Promise<number> {
  return await invoke<number>("get_master_volume");
}
export async function setMasterVolume(volume: number): Promise<number> {
  return await invoke<number>("set_master_volume", { volume });
}

export async function setSessionType(sessionType: SessionType): Promise<void> {
  await invoke("set_session_type", { sessionType });
}

export async function getSnapshot(): Promise<SessionSnapshot> {
  return await invoke<SessionSnapshot>("get_snapshot");
}

export async function getProvenance(): Promise<Provenance> {
  return await invoke<Provenance>("get_provenance");
}

export async function getCurrentSource(): Promise<CurrentSource> {
  return await invoke<CurrentSource>("get_current_source");
}

export async function getItemFeedback(itemId: string): Promise<ItemFeedbackState> {
  return await invoke<ItemFeedbackState>("get_item_feedback", { itemId });
}

export async function setItemFeedback(
  itemId: string,
  focusFeedback: TrackFeedback | null,
  enjoyment: TrackEnjoyment | null,
): Promise<ItemFeedbackState> {
  return await invoke<ItemFeedbackState>("set_item_feedback", {
    itemId,
    focusFeedback,
    enjoyment,
  });
}

export async function listContentPacks(): Promise<ContentPackSummary[]> {
  return await invoke<ContentPackSummary[]>("list_content_packs");
}

export async function chooseAndImportContentPack(): Promise<ContentPackSummary | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Aria Focus content pack", extensions: ["adhdpack"] }],
  });
  if (selected === null) return null;
  return await invoke<ContentPackSummary>("import_content_pack", { path: selected });
}
