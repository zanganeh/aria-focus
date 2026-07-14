import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

import {
  chooseAndImportContentPack,
  completeOnboarding,
  getActivityGenres,
  getActivityMoods,
  getItemFeedback,
  getMasterVolume,
  getOnboardingPreferences,
  getStartupHealth,
  nextTrack,
  previousTrack,
  retryStartup,
  setActivityGenre,
  setActivityMood,
  setItemFeedback,
  setMasterVolume,
  listFavorites,
  listRecentSessions,
  removeFavorite,
  saveSessionRating,
  startFavorite,
  getStudioCapability,
  getStudioJob,
  listRecentStudioJobs,
  createStudioMusic,
  cancelStudioMusic,
  cancelRuntimeInstall,
  deleteMyMusic,
  discardStudioDraft,
  getDraftPreviewState,
  getRuntimeInstall,
  listMyMusic,
  pauseDraftPreview,
  renameMyMusic,
  repairRuntime,
  resumeDraftPreview,
  saveStudioDraft,
  startDraftPreview,
  startMyMusic,
  startRuntimeInstall,
  stopDraftPreview,
} from "./api";

beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.open.mockReset();
});

describe("mood API", () => {
  it("uses the current-activity commands and sends null for Any", async () => {
    mocks.invoke.mockResolvedValue({
      selected_mood_id: null,
      available_moods: [],
      selected_mood_available: true,
    });
    await getActivityMoods();
    await setActivityMood(null);
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_activity_moods");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "set_activity_mood", { moodId: null });
  });
});

describe("Music Studio API", () => {
  it("maps the read-only capability and saved-item commands", async () => {
    mocks.invoke.mockResolvedValue([]);
    await getStudioCapability();
    await listRecentStudioJobs();
    await getStudioJob("job_abcdefghijkl");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_studio_capability");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "list_recent_studio_jobs");
    expect(mocks.invoke).toHaveBeenNthCalledWith(3, "get_studio_job", {
      jobId: "job_abcdefghijkl",
    });
  });

  it("maps create and cancel payloads exactly", async () => {
    mocks.invoke.mockResolvedValue({ id: "job_abcdefghijkl" });
    const request = {
      activity: "deep_work" as const,
      sound_style_id: "ambient" as const,
      energy: "medium" as const,
      duration_seconds: 180 as const,
      note: "soft rain",
      parent_job_id: null,
    };
    await createStudioMusic(request);
    await cancelStudioMusic("job_abcdefghijkl");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "create_studio_music", { request });
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "cancel_studio_music", {
      jobId: "job_abcdefghijkl",
    });
  });

  it("maps setup, preview, save, discard, and My Music commands exactly", async () => {
    mocks.invoke.mockResolvedValue(undefined);
    await getRuntimeInstall();
    await startRuntimeInstall();
    await cancelRuntimeInstall();
    await repairRuntime();
    await getDraftPreviewState();
    await startDraftPreview("job_abcdefghijkl");
    await pauseDraftPreview();
    await resumeDraftPreview();
    await stopDraftPreview();
    await saveStudioDraft("job_abcdefghijkl", "Steady rain");
    await discardStudioDraft("job_abcdefghijkl");
    await listMyMusic();
    await renameMyMusic("generated.local.job_abcdefghijkl.item", "Rain");
    await deleteMyMusic("generated.local.job_abcdefghijkl.item");
    await startMyMusic("generated.local.job_abcdefghijkl.item", "deep_work");

    expect(mocks.invoke.mock.calls).toEqual([
      ["get_runtime_install"],
      ["start_runtime_install"],
      ["cancel_runtime_install"],
      ["repair_runtime"],
      ["get_draft_preview_state"],
      ["start_draft_preview", { jobId: "job_abcdefghijkl" }],
      ["pause_draft_preview"],
      ["resume_draft_preview"],
      ["stop_draft_preview"],
      ["save_studio_draft", { jobId: "job_abcdefghijkl", title: "Steady rain" }],
      ["discard_studio_draft", { jobId: "job_abcdefghijkl" }],
      ["list_my_music"],
      ["rename_my_music", { itemId: "generated.local.job_abcdefghijkl.item", title: "Rain" }],
      ["delete_my_music", { itemId: "generated.local.job_abcdefghijkl.item" }],
      [
        "start_my_music",
        { itemId: "generated.local.job_abcdefghijkl.item", activity: "deep_work" },
      ],
    ]);
  });
});

describe("favorites API", () => {
  it("uses local list, exact activity-scoped removal, and direct start commands", async () => {
    mocks.invoke.mockResolvedValue([]);
    await listFavorites();
    await removeFavorite("rain", "deep_work");
    await startFavorite("rain", "deep_work");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "list_favorites");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "remove_favorite", {
      itemId: "rain",
      activity: "deep_work",
    });
    expect(mocks.invoke).toHaveBeenNthCalledWith(3, "start_favorite", {
      itemId: "rain",
      activity: "deep_work",
    });
  });
});

describe("master-volume API", () => {
  it("uses global get/set commands with a numeric percent", async () => {
    mocks.invoke.mockResolvedValue(70);
    await getMasterVolume();
    await setMasterVolume(42);
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_master_volume");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "set_master_volume", { volume: 42 });
  });
});

describe("installed-track navigation API", () => {
  it("uses distinct next and previous commands", async () => {
    mocks.invoke.mockResolvedValue(undefined);
    await nextTrack();
    await previousTrack();
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "next_track");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "previous_track");
  });
});

describe("onboarding API", () => {
  it("loads local state and sends the selected intensity and genres", async () => {
    mocks.invoke.mockResolvedValue({ completed: false, intensity: "medium", genres: [] });
    await getOnboardingPreferences();
    await completeOnboarding("high", ["drone", "nature"]);
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_onboarding_preferences");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "complete_onboarding", {
      intensity: "high",
      genres: ["drone", "nature"],
    });
  });
});

describe("session-history API", () => {
  it("lists recent sessions and updates only the requested session ratings", async () => {
    mocks.invoke.mockResolvedValue([]);
    await listRecentSessions();
    await saveSessionRating("0123456789abcdef0123456789abcdef", "neutral", "liked");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "list_recent_sessions");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "save_session_rating", {
      id: "0123456789abcdef0123456789abcdef",
      focusOutcome: "neutral",
      soundEnjoyment: "liked",
    });
  });
});

describe("genre API", () => {
  it("uses the current-activity commands and sends null for Any", async () => {
    mocks.invoke.mockResolvedValue({
      selected_genre_id: null,
      available_genres: [],
      selected_genre_available: true,
    });
    await getActivityGenres();
    await setActivityGenre(null);
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_activity_genres");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "set_activity_genre", { genreId: null });
  });
});

describe("startup recovery API", () => {
  it("uses explicit health and retry commands", async () => {
    mocks.invoke.mockResolvedValue({
      core_ready: false,
      core_error: "no audio device",
      packs_ready: true,
      packs_error: null,
    });
    await getStartupHealth();
    await retryStartup();
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_startup_health");
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "retry_startup");
  });
});

describe("track feedback API", () => {
  it("passes the displayed item to the feedback commands", async () => {
    mocks.invoke.mockResolvedValue({
      item_id: "track",
      activity: "deep_work",
      focus_feedback: null,
      enjoyment: null,
    });
    await getItemFeedback("track");
    await setItemFeedback("track", "helps_focus", "liked");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_item_feedback", { itemId: "track" });
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "set_item_feedback", {
      itemId: "track",
      focusFeedback: "helps_focus",
      enjoyment: "liked",
    });
  });
});

describe("chooseAndImportContentPack", () => {
  it("uses the official chooser with a single adhdpack filter", async () => {
    mocks.open.mockResolvedValue("C:\\private\\fixture.adhdpack");
    mocks.invoke.mockResolvedValue({ id: "fixture.pack" });

    await chooseAndImportContentPack();

    expect(mocks.open).toHaveBeenCalledWith({
      multiple: false,
      directory: false,
      filters: [{ name: "Aria Focus content pack", extensions: ["adhdpack"] }],
    });
    expect(mocks.invoke).toHaveBeenCalledWith("import_content_pack", {
      path: "C:\\private\\fixture.adhdpack",
    });
  });

  it("does not invoke the backend when the chooser is cancelled", async () => {
    mocks.open.mockResolvedValue(null);
    await expect(chooseAndImportContentPack()).resolves.toBeNull();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});
