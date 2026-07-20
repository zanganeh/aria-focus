import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import { findAvailableUpdate, installAndRelaunch } from "./lib/updater";
import {
  completeOnboarding,
  getActivityGenres,
  getActivityMoods,
  getCurrentSource,
  getItemFeedback,
  getProvenance,
  getStartupHealth,
  getOnboardingPreferences,
  listReviewCandidates,
  listFavorites,
  removeFavorite,
  retryStartup,
  startFavorite,
  startReviewCandidate,
  listRecentSessions,
  saveSessionRating,
  listMyMusic,
  startMyMusic,
  getStudioCapability,
} from "./lib/api";

const mockSession = vi.hoisted(() => ({
  snapshot: {
    activity: "deep_work",
    intensity: "medium",
    status: "playing",
    kind: { kind: "infinite" },
    phase: "work",
    current_round: null,
    total_rounds: null,
    focus_elapsed_seconds: 1,
    current_phase_remaining_seconds: null,
    total_remaining_seconds: null,
  },
  intensity: "medium",
  starting: false,
  error: null,
  start: vi.fn(),
  pause: vi.fn(),
  resume: vi.fn(),
  stop: vi.fn(),
  changeActivity: vi.fn(),
  changeIntensity: vi.fn(),
  changeSessionType: vi.fn(),
  dismissError: vi.fn(),
  reportError: vi.fn(),
  refresh: vi.fn(),
  adoptStartedSession: vi.fn(),
  clearSessionLoadError: vi.fn(),
  masterVolume: 70,
  volumePending: false,
  changeMasterVolume: vi.fn(),
}));

vi.mock("./hooks/useSession", () => ({
  useSession: () => mockSession,
}));

vi.mock("./lib/updater", () => ({
  findAvailableUpdate: vi.fn(),
  installAndRelaunch: vi.fn(),
}));

vi.mock("./lib/api", () => ({
  listMyMusic: vi.fn().mockResolvedValue([]),
  startMyMusic: vi.fn().mockResolvedValue(undefined),
  renameMyMusic: vi.fn().mockResolvedValue(undefined),
  deleteMyMusic: vi.fn().mockResolvedValue(undefined),
  getCurrentSource: vi.fn(),
  getProvenance: vi.fn(),
  getActivityGenres: vi.fn(),
  getActivityMoods: vi.fn(),
  setActivityMood: vi.fn(),
  getItemFeedback: vi.fn(),
  setItemFeedback: vi.fn(),
  setActivityGenre: vi.fn(),
  getStartupHealth: vi.fn(),
  retryStartup: vi.fn(),
  listReviewCandidates: vi.fn(),
  startReviewCandidate: vi.fn(),
  getOnboardingPreferences: vi.fn(),
  completeOnboarding: vi.fn(),
  listFavorites: vi.fn(),
  removeFavorite: vi.fn(),
  startFavorite: vi.fn(),
  listRecentSessions: vi.fn(),
  saveSessionRating: vi.fn(),
  getStudioCapability: vi.fn(),
  listRecentStudioJobs: vi.fn().mockResolvedValue([]),
  getRuntimeInstall: vi.fn().mockResolvedValue({ status: "idle", stage: "waiting", detail: "" }),
  getDraftPreviewState: vi.fn().mockResolvedValue("stopped"),
  stopDraftPreview: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("./components/ContentPacks", () => ({
  ContentPacks: ({
    onCatalogueChange,
    disabled,
  }: {
    onCatalogueChange?: () => void;
    disabled?: boolean;
  }) => (
    <button type="button" onClick={onCatalogueChange} disabled={disabled}>
      Refresh music catalogue
    </button>
  ),
}));

const EMPTY_GENRES = {
  selected_genre_id: null,
  selected_genre_available: true,
  available_genres: [],
};
const EMPTY_MOODS = { selected_mood_id: null, selected_mood_available: true, available_moods: [] };

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(findAvailableUpdate).mockResolvedValue(null);
  vi.mocked(installAndRelaunch).mockResolvedValue();
  mockSession.snapshot = {
    activity: "deep_work",
    intensity: "medium",
    status: "playing",
    kind: { kind: "infinite" },
    phase: "work",
    current_round: null,
    total_rounds: null,
    focus_elapsed_seconds: 1,
    current_phase_remaining_seconds: null,
    total_remaining_seconds: null,
  };
  vi.mocked(getProvenance).mockReset();
  vi.mocked(getActivityGenres).mockReset();
  vi.mocked(getActivityMoods).mockReset();
  vi.mocked(getCurrentSource).mockReset();
  vi.mocked(getItemFeedback).mockReset();
  vi.mocked(getProvenance).mockResolvedValue(null as never);
  vi.mocked(getActivityGenres).mockResolvedValue(EMPTY_GENRES);
  vi.mocked(getActivityMoods).mockResolvedValue(EMPTY_MOODS);
  vi.mocked(getCurrentSource)
    .mockResolvedValueOnce({
      pack_id: "pack-a",
      pack_title: "Pack A",
      item_id: "item-a",
      item_title: "First Track",
      variant_id: "base",
      fallback: false,
      cover_art: "data:image/png;base64,ZmFrZQ==",
    })
    .mockResolvedValue({
      pack_id: "pack-b",
      pack_title: "Pack B",
      item_id: "item-b",
      item_title: "Second Track",
      variant_id: "base",
      fallback: false,
    });
  vi.mocked(getItemFeedback).mockResolvedValue({
    item_id: "item-a",
    activity: "deep_work",
    focus_feedback: null,
    enjoyment: null,
  });
  vi.mocked(getStartupHealth).mockResolvedValue({
    core_ready: true,
    core_error: null,
    packs_ready: true,
    packs_error: null,
  });
  vi.mocked(getOnboardingPreferences).mockResolvedValue({
    completed: true,
    intensity: "medium",
    genres: [],
  });
  vi.mocked(retryStartup).mockResolvedValue({
    core_ready: true,
    core_error: null,
    packs_ready: true,
    packs_error: null,
  });
  vi.mocked(listReviewCandidates).mockResolvedValue([]);
  vi.mocked(startReviewCandidate).mockResolvedValue();
  vi.mocked(listFavorites).mockResolvedValue([]);
  vi.mocked(removeFavorite).mockResolvedValue();
  vi.mocked(startFavorite).mockResolvedValue();
  vi.mocked(listRecentSessions).mockResolvedValue([]);
  vi.mocked(saveSessionRating).mockResolvedValue();
  vi.mocked(listMyMusic).mockResolvedValue([]);
  vi.mocked(startMyMusic).mockResolvedValue();
  vi.mocked(getStudioCapability).mockResolvedValue({ state: "ready", detail: null });
});

it("shows an available update without installing until the user consents", async () => {
  const update = {
    version: "0.4.0",
    body: "A safer, smoother release.",
  } as never;
  vi.mocked(findAvailableUpdate).mockResolvedValue(update);

  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("heading", { name: "Aria Focus 0.4.0 is ready" })).toBeTruthy();
  expect(installAndRelaunch).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Download and restart" }));
  await act(async () => Promise.resolve());
  expect(installAndRelaunch).toHaveBeenCalledWith(update);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

it("refreshes the callback-published source identity while playback is active", async () => {
  render(<App />);
  await act(async () => Promise.resolve());
  expect(screen.getByText(/First Track/)).toBeTruthy();

  await act(async () => vi.advanceTimersByTimeAsync(500));
  expect(screen.getByText(/Second Track/)).toBeTruthy();
  expect(getCurrentSource).toHaveBeenCalledTimes(2);
});

it("uses approved cover art as player and focus-view background only", async () => {
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Open player" }));
  expect(
    screen.getByRole("region", { name: "Focus player" }).querySelector(".player-background"),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Enter focus view" }));
  expect(
    screen.getByRole("main", { name: "Focus view" }).querySelector(".focus-view-background"),
  ).toBeTruthy();
});

it.each(["stopped", "expired"] as const)(
  "leaves focus view when the session becomes %s",
  async (status) => {
    const { rerender } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "Open player" }));
    const entry = screen.getByRole("button", { name: "Enter focus view" });
    fireEvent.click(entry);
    expect(screen.getByRole("main", { name: "Focus view" })).toBeTruthy();

    mockSession.snapshot = { ...mockSession.snapshot, status };
    rerender(<App />);
    await act(async () => Promise.resolve());
    expect(screen.queryByRole("main", { name: "Focus view" })).toBeNull();
  },
);

it("restores focus to the entry control when leaving focus view", async () => {
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Open player" }));
  const entry = screen.getByRole("button", { name: "Enter focus view" });
  fireEvent.click(entry);
  fireEvent.keyDown(screen.getByRole("button", { name: "Pause" }), { key: "Escape" });
  await act(async () => vi.advanceTimersByTimeAsync(20));
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Enter focus view" }));
});

it("refreshes genres after a catalogue change and ignores a superseded response", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  let resolveInitial: (genres: typeof EMPTY_GENRES) => void;
  const initialGenres = new Promise<typeof EMPTY_GENRES>((resolve) => {
    resolveInitial = resolve;
  });
  const importedGenres = {
    selected_genre_id: null,
    selected_genre_available: true,
    available_genres: [{ id: "ambient", label: "Ambient" }],
  };
  vi.mocked(getActivityGenres)
    .mockImplementationOnce(() => initialGenres)
    .mockResolvedValueOnce(importedGenres);

  render(<App />);
  await act(async () => Promise.resolve());
  expect(getActivityGenres).toHaveBeenCalledOnce();

  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh music catalogue" }));
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("radio", { name: "Ambient" })).toBeTruthy();

  await act(async () => resolveInitial!(EMPTY_GENRES));
  expect(screen.getByRole("radio", { name: "Ambient" })).toBeTruthy();
});

it("shows startup recovery only for failed services and keeps partial failure visible", async () => {
  vi.mocked(getStartupHealth).mockResolvedValue({
    core_ready: true,
    core_error: null,
    packs_ready: false,
    packs_error: "content registry is locked",
  });
  vi.mocked(retryStartup).mockResolvedValue({
    core_ready: true,
    core_error: null,
    packs_ready: false,
    packs_error: "content registry is still locked",
  });
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByText(/Installed content packs are unavailable/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Retry startup" }));
  await act(async () => Promise.resolve());
  expect(screen.getByText(/still locked/)).toBeTruthy();
});

it("gates core-dependent controls while core startup is failed", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  vi.mocked(getStartupHealth).mockResolvedValue({
    core_ready: false,
    core_error: "audio unavailable",
    packs_ready: true,
    packs_error: null,
  });
  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("button", { name: "Start Deep Work" }).hasAttribute("disabled")).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(
    screen.getByRole("group", { name: "Stimulation intensity" }).hasAttribute("disabled"),
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(
    screen.getByRole("button", { name: "Refresh music catalogue" }).hasAttribute("disabled"),
  ).toBe(false);
  expect(screen.queryByText(/How is this track/)).toBeNull();
});

it("hides feedback and exposes the provisional review warning for a review source", async () => {
  vi.mocked(getCurrentSource).mockReset();
  vi.mocked(getCurrentSource).mockResolvedValue({
    pack_id: "unrelated-pack-id",
    pack_title: "Review",
    item_id: "candidate-e",
    item_title: "Track E — quarantined review",
    variant_id: "pinned",
    fallback: false,
    quarantined_review: true,
  });
  vi.mocked(listReviewCandidates).mockResolvedValue([
    {
      alias: "E",
      title: "Track E",
      review_id: "E",
      bytes: 1,
      codec: "FLAC",
      sample_rate_hz: 48000,
      channels: 2,
      duration_seconds: 90,
      quarantine_status: "local_evaluation_only_not_approved_or_published_provisional_transition",
    },
  ]);
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: /Review local music/ }));
  expect(screen.getByText(/provisional boundary crossfade/i)).toBeTruthy();
  expect(screen.queryByText(/How is this track/)).toBeNull();
});

it("renders the active installed item's cover art in the player surface", async () => {
  vi.mocked(getCurrentSource).mockReset();
  vi.mocked(getCurrentSource).mockResolvedValue({
    pack_id: "pack-a",
    pack_title: "Pack A",
    item_id: "item-a",
    item_title: "First Track",
    variant_id: "base",
    fallback: false,
    cover_art: "data:image/png;base64,aGVsbG8=",
  });
  mockSession.snapshot = { ...mockSession.snapshot, status: "playing" };
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Open player" }));

  const player = screen.getByRole("region", { name: "Focus player" });
  const cover = within(player).getByRole("img", { name: "First Track cover art" });
  expect(cover.getAttribute("src")).toBe("data:image/png;base64,aGVsbG8=");
});

it("shows a graceful no-cover fallback and never renders cover art for a fallback source", async () => {
  vi.mocked(getCurrentSource).mockReset();
  vi.mocked(getCurrentSource).mockResolvedValue({
    pack_id: "bundled-test-source",
    pack_title: "Bundled fallback",
    item_id: "procedural-focus-tone",
    item_title: "Procedural test tone",
    variant_id: "deterministic-v1",
    fallback: true,
    cover_art: "data:image/png;base64,aGVsbG8=",
  });
  mockSession.snapshot = { ...mockSession.snapshot, status: "playing" };
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Open player" }));

  const player = screen.getByRole("region", { name: "Focus player" });
  expect(within(player).getByRole("img", { name: "Deep Work artwork" })).toBeTruthy();
  expect(within(player).getByText(/Procedural test tone/)).toBeTruthy();
});

it("gates pack-dependent controls while packs startup is failed", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  vi.mocked(getStartupHealth).mockResolvedValue({
    core_ready: true,
    core_error: null,
    packs_ready: false,
    packs_error: "registry unavailable",
  });
  render(<App />);
  await act(async () => Promise.resolve());

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("group", { name: "Music genre" }).hasAttribute("disabled")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(
    screen.getByRole("button", { name: "Refresh music catalogue" }).hasAttribute("disabled"),
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Home" }));
  expect(screen.getByRole("button", { name: "Start Deep Work" }).hasAttribute("disabled")).toBe(
    true,
  );
  expect(screen.queryByText(/How is this track/)).toBeNull();
});

it("enables only the subsystem recovered by retry, then restores all controls", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  vi.mocked(getStartupHealth).mockResolvedValue({
    core_ready: false,
    core_error: "audio unavailable",
    packs_ready: false,
    packs_error: "registry unavailable",
  });
  vi.mocked(retryStartup)
    .mockResolvedValueOnce({
      core_ready: true,
      core_error: null,
      packs_ready: false,
      packs_error: "registry unavailable",
    })
    .mockResolvedValueOnce({
      core_ready: true,
      core_error: null,
      packs_ready: true,
      packs_error: null,
    });
  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("button", { name: "Start Deep Work" }).hasAttribute("disabled")).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(
    screen.getByRole("button", { name: "Refresh music catalogue" }).hasAttribute("disabled"),
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Retry startup" }));
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Home" }));
  expect(screen.getByRole("button", { name: "Start Deep Work" }).hasAttribute("disabled")).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(
    screen.getByRole("button", { name: "Refresh music catalogue" }).hasAttribute("disabled"),
  ).toBe(true);

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Retry startup" }));
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(
    screen.getByRole("button", { name: "Refresh music catalogue" }).hasAttribute("disabled"),
  ).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("group", { name: "Music genre" }).hasAttribute("disabled")).toBe(false);
});

it("shows fresh onboarding and leaves it only after backend completion", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  vi.mocked(getOnboardingPreferences).mockResolvedValue({
    completed: false,
    intensity: "medium",
    genres: [],
  });
  vi.mocked(completeOnboarding).mockResolvedValue();

  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("heading", { name: "Set a starting sound preference" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Start 30-minute Deep Work" }));
  await act(async () => Promise.resolve());

  expect(completeOnboarding).toHaveBeenCalledWith("medium", []);
  expect(screen.getByRole("button", { name: "Start Deep Work" })).toBeTruthy();
});

it("bypasses onboarding only for a build with local quarantined review resources", async () => {
  vi.mocked(getOnboardingPreferences).mockResolvedValue({
    completed: false,
    intensity: "medium",
    genres: [],
  });
  vi.mocked(listReviewCandidates).mockResolvedValue([
    {
      alias: "I",
      title: "Track I",
      review_id: "I",
      bytes: 1,
      codec: "FLAC",
      sample_rate_hz: 48000,
      channels: 2,
      duration_seconds: 90,
      quarantine_status: "local_evaluation_only_not_approved_or_published",
    },
  ]);

  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.queryByRole("heading", { name: "Set a starting sound preference" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: /Review local music/ }));
  expect(screen.getByRole("heading", { name: "Quarantined candidate review" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start quarantined review Track I" })).toBeTruthy();
});

it("does not bypass onboarding when local preferences fail to load and supports retry", async () => {
  vi.mocked(getOnboardingPreferences)
    .mockRejectedValueOnce(new Error("database unavailable"))
    .mockResolvedValueOnce({ completed: false, intensity: "medium", genres: [] });

  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("heading", { name: "Couldn’t load local preferences" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Start Deep Work" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await act(async () => Promise.resolve());

  expect(getOnboardingPreferences).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("heading", { name: "Set a starting sound preference" })).toBeTruthy();
});

it("shows completed sessions in history without asking for feedback", async () => {
  const completed = {
    id: "0123456789abcdef0123456789abcdef",
    activity: "deep_work" as const,
    intensity: "medium" as const,
    session_type: { kind: "infinite" as const },
    started_at: 100,
    ended_at: 160,
    end_reason: "stopped" as const,
    focus_seconds: 60,
    focus_outcome: null,
    sound_enjoyment: null,
  };
  vi.mocked(listRecentSessions).mockResolvedValue([completed]);
  const view = render(<App />);
  await act(async () => Promise.resolve());

  mockSession.snapshot = { ...mockSession.snapshot, status: "stopped" };
  view.rerender(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "History" }));
  expect(screen.queryByRole("heading", { name: "How was that session?" })).toBeNull();
  expect(screen.getByText(/Deep Work/)).toBeTruthy();
});

it("uses clear pages and keeps an active-session route back to the player", async () => {
  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("button", { name: "Home" }).getAttribute("aria-current")).toBe("page");
  fireEvent.click(screen.getByRole("button", { name: "Library" }));

  expect(screen.getByRole("button", { name: "Library" }).getAttribute("aria-current")).toBe("page");
  expect(screen.getByRole("button", { name: "Open player" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Pause session" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Start Deep Work" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Open player" }));
  expect(screen.getByRole("region", { name: "Focus player" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Active focus session" })).toBeNull();
});

it("opens Music Studio from the simple Library card", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  render(<App />);
  await act(async () => Promise.resolve());

  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  fireEvent.click(screen.getByRole("button", { name: /Create your focus music/ }));
  await act(async () => Promise.resolve());

  expect(screen.getByRole("heading", { name: "Create your focus music" })).toBeTruthy();
});

it("makes music creation directly discoverable in the main navigation", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  render(<App />);
  await act(async () => Promise.resolve());

  const create = screen.getByRole("button", { name: "Create" });
  fireEvent.click(create);
  await act(async () => Promise.resolve());

  expect(create.getAttribute("aria-current")).toBe("page");
  expect(screen.getByRole("heading", { name: "Create your focus music" })).toBeTruthy();
});

it("adopts My Music playback and returns to the player", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  vi.mocked(listMyMusic).mockResolvedValue([
    {
      item_id: "generated.local.job_abcdefghijkl.item",
      title: "Steady rain",
      duration_seconds: 180,
      created_at: 1,
      activity: "deep_work",
      job_id: "job_abcdefghijkl",
    },
  ]);
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  await act(async () => Promise.resolve());

  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await act(async () => Promise.resolve());

  expect(startMyMusic).toHaveBeenCalledWith("generated.local.job_abcdefghijkl.item", "deep_work");
  expect(mockSession.adoptStartedSession).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Home" }).getAttribute("aria-current")).toBe("page");
});

it("exposes the live master volume on the active player and routes changes through the session", async () => {
  mockSession.masterVolume = 64;
  render(<App />);
  await act(async () => Promise.resolve());
  fireEvent.click(screen.getByRole("button", { name: "Open player" }));

  const slider = screen.getByRole("slider", { name: /Master volume/ }) as HTMLInputElement;
  expect(slider.value).toBe("64");
  expect(slider.disabled).toBe(false);
  fireEvent.change(slider, { target: { value: "65" } });
  expect(mockSession.changeMasterVolume).toHaveBeenCalledWith(65);
});

it("starts directly from a tile and keeps optional controls in bottom settings", async () => {
  mockSession.snapshot = { ...mockSession.snapshot, status: "idle" };
  render(<App />);
  await act(async () => Promise.resolve());

  expect(screen.getByRole("button", { name: "Start Deep Work" })).toBeTruthy();
  expect(screen.queryByRole("group", { name: "Music genre" })).toBeNull();
  expect(screen.queryByRole("group", { name: "Session timer" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Start Creativity" }));
  await act(async () => Promise.resolve());
  expect(mockSession.changeActivity).toHaveBeenCalledWith("creativity");
  expect(mockSession.start).toHaveBeenCalledOnce();
  expect(screen.getByRole("region", { name: "Focus player" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Choose a focus activity" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("heading", { name: "Sound and timer" })).toBeTruthy();
  expect(screen.getByRole("group", { name: "Music genre" })).toBeTruthy();
  expect(screen.getByRole("group", { name: "Session timer" })).toBeTruthy();
});
