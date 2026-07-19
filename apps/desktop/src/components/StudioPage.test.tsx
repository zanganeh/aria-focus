import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { StudioJobSummary } from "../lib/types";
import { StudioPage } from "./StudioPage";

const api = vi.hoisted(() => ({
  getStudioCapability: vi.fn(),
  getRuntimeInstall: vi.fn(),
  listRecentStudioJobs: vi.fn(),
  startRuntimeInstall: vi.fn(),
  cancelRuntimeInstall: vi.fn(),
  repairRuntime: vi.fn(),
  createStudioMusic: vi.fn(),
  cancelStudioMusic: vi.fn(),
  getDraftPreviewState: vi.fn(),
  startDraftPreview: vi.fn(),
  pauseDraftPreview: vi.fn(),
  resumeDraftPreview: vi.fn(),
  stopDraftPreview: vi.fn(),
  saveStudioDraft: vi.fn(),
  discardStudioDraft: vi.fn(),
  regenerateStudioMusic: vi.fn(),
}));
vi.mock("../lib/api", () => api);

function job(overrides: Partial<StudioJobSummary> = {}): StudioJobSummary {
  return {
    id: "job_abcdefghijkl",
    status: "In progress",
    updated_at_ms: 10,
    length_seconds: 180,
    stage: "creating",
    can_preview: false,
    can_save: false,
    can_discard: true,
    safe_message: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getStudioCapability.mockResolvedValue({ state: "ready", detail: null });
  api.listRecentStudioJobs.mockResolvedValue([]);
  api.getRuntimeInstall.mockResolvedValue({ status: "idle", stage: "waiting", detail: "" });
  api.createStudioMusic.mockResolvedValue(job({ stage: "preparing" }));
  api.cancelStudioMusic.mockResolvedValue(
    job({ status: "Needs attention", stage: "complete", can_discard: false }),
  );
  api.getDraftPreviewState.mockResolvedValue("stopped");
  api.startDraftPreview.mockResolvedValue(undefined);
  api.pauseDraftPreview.mockResolvedValue(undefined);
  api.resumeDraftPreview.mockResolvedValue(undefined);
  api.stopDraftPreview.mockResolvedValue(undefined);
  api.saveStudioDraft.mockResolvedValue({
    item_id: "generated.local.job_abcdefghijkl.item",
    title: "My focus music",
    duration_seconds: 180,
    created_at: 1,
    activity: "deep_work",
    job_id: "job_abcdefghijkl",
  });
  api.discardStudioDraft.mockResolvedValue(undefined);
  api.regenerateStudioMusic.mockResolvedValue(job({ id: "job_mnopqrstuvwx", stage: "preparing" }));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it("enables Generate with defaults, enforces the note limit, and sends the exact request", async () => {
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);
  const generate = await screen.findByRole("button", { name: "Generate" });
  expect(generate.hasAttribute("disabled")).toBe(false);
  expect((screen.getByLabelText("Focus type") as HTMLSelectElement).value).toBe("deep_work");
  expect((screen.getByLabelText("Genre") as HTMLSelectElement).value).toBe("ambient");
  expect((screen.getByLabelText("Mood") as HTMLSelectElement).value).toBe("focused");
  expect((screen.getByLabelText("Energy") as HTMLSelectElement).value).toBe("medium");
  expect((screen.getByLabelText("Speed / tempo") as HTMLSelectElement).value).toBe("90");
  expect((screen.getByLabelText("Duration") as HTMLSelectElement).value).toBe("180");
  const details = screen.getByLabelText("Details");
  expect(details.getAttribute("maxlength")).toBe("500");
  await user.type(details, "soft rain");
  await user.click(generate);
  expect(api.createStudioMusic).toHaveBeenCalledWith({
    activity: "deep_work",
    genre_id: "ambient",
    sound_style_id: "ambient",
    mood_id: "focused",
    energy: "medium",
    tempo_bpm: 90,
    duration_seconds: 180,
    instrument_ids: [],
    additional_details: "soft rain",
    creative_direction: null,
    parent_job_id: null,
  });
});

it("restores an active job, renders its customer stage, and waits for cancel", async () => {
  const active = job({ stage: "checking" });
  api.listRecentStudioJobs.mockResolvedValue([active]);
  api.cancelStudioMusic.mockResolvedValue(
    job({ status: "Needs attention", stage: "complete", can_discard: false }),
  );
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);
  expect(await screen.findByText(/Checking your music/)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(api.cancelStudioMusic).toHaveBeenCalledWith(active.id));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull());
});

it("keeps advanced controls collapsed and sends bounded creative choices", async () => {
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);
  await screen.findByRole("button", { name: "Generate" });
  expect(screen.queryByLabelText("Fuller creative direction")).toBeNull();
  await user.click(screen.getByRole("button", { name: /More/ }));
  await user.click(screen.getByLabelText("Piano"));
  await user.selectOptions(screen.getByLabelText("Genre"), "electronic");
  await user.selectOptions(screen.getByLabelText("Mood"), "calm");
  await user.selectOptions(screen.getByLabelText("Energy"), "high");
  await user.type(screen.getByLabelText("Details"), "steady rain");
  await user.type(
    screen.getByLabelText("Fuller creative direction"),
    "A patient, spacious arrangement",
  );
  await user.selectOptions(screen.getByLabelText("Speed / tempo"), "150");
  const fullPrompt =
    "instrumental music only; activity: deep_work; genre: electronic; energy: high; tempo: 150 BPM; mood: calm; instruments: piano; details: steady rain; creative direction: A patient, spacious arrangement";
  expect(screen.getByText(fullPrompt).tagName).toBe("PRE");
  expect(screen.getByText("Full prompt preview")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Generate" }));
  expect(api.createStudioMusic).toHaveBeenCalledWith(
    expect.objectContaining({
      genre_id: "electronic",
      sound_style_id: "electronic",
      mood_id: "calm",
      energy: "high",
      tempo_bpm: 150,
      instrument_ids: ["piano"],
      additional_details: "steady rain",
      creative_direction: "A patient, spacious arrangement",
    }),
  );
});

it("shows the complete deterministic prompt on result cards", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({
      status: "Ready",
      stage: "ready",
      can_preview: true,
      can_save: true,
      creative_prompt: "instrumental music only; genre: ambient; tempo: 90 BPM",
      locked_negative_prompt: "vocals, lyrics, spoken words",
    }),
  ]);
  render(<StudioPage onReturn={vi.fn()} />);
  await userEvent.click(await screen.findByText("Generated prompt"));
  expect(screen.getByText(/genre: ambient; tempo: 90 BPM/)).toBeTruthy();
  expect(screen.getByText(/vocals, lyrics, spoken words/)).toBeTruthy();
});

it("polling survives reload state and refreshes an active job to ready", async () => {
  const active = job();
  const ready = job({
    status: "Ready",
    stage: "ready",
    can_preview: true,
    can_save: true,
  });
  api.listRecentStudioJobs.mockResolvedValueOnce([active]).mockResolvedValue([ready]);
  vi.spyOn(window, "setInterval").mockImplementation((handler: TimerHandler) => {
    queueMicrotask(() => (handler as () => void)());
    return 1 as unknown as ReturnType<typeof window.setInterval>;
  });
  render(<StudioPage onReturn={vi.fn()} />);
  expect(await screen.findByText("Ready")).toBeTruthy();
  await waitFor(() => expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull());
});

it("previews, pauses, resumes, and keeps Save available once preview starts", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({ status: "Ready", stage: "ready", can_preview: true, can_save: true }),
  ]);
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: "Preview" }));
  expect(api.startDraftPreview).toHaveBeenCalledWith("job_abcdefghijkl");
  expect(screen.getByRole("button", { name: "Save to My Music" }).hasAttribute("disabled")).toBe(
    false,
  );

  await user.click(screen.getByRole("button", { name: "Pause preview" }));
  expect(api.pauseDraftPreview).toHaveBeenCalledOnce();
  await user.click(screen.getByRole("button", { name: "Resume preview" }));
  expect(api.resumeDraftPreview).toHaveBeenCalledOnce();
  await user.click(screen.getByRole("button", { name: "Stop preview" }));
  expect(api.stopDraftPreview).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Preview" })).toBeTruthy();

  vi.spyOn(window, "prompt").mockReturnValue("Deep current");
  await user.click(screen.getByRole("button", { name: "Save to My Music" }));
  expect(api.saveStudioDraft).toHaveBeenCalledWith("job_abcdefghijkl", "Deep current");
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Save to My Music" })).toBeNull(),
  );
});

it("keeps preview controls scoped to the selected draft", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({ status: "Ready", stage: "ready", can_preview: true, can_save: true }),
    job({
      id: "job_mnopqrstuvwx",
      status: "Ready",
      stage: "ready",
      can_preview: true,
      can_save: true,
    }),
  ]);
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);

  const previews = await screen.findAllByRole("button", { name: "Preview" });
  await user.click(previews[0]);

  expect(screen.getAllByRole("button", { name: "Pause preview" })).toHaveLength(1);
  expect(screen.getAllByRole("button", { name: "Preview" })).toHaveLength(1);
});

it("stops draft preview before returning to the Library", async () => {
  const onReturn = vi.fn();
  render(<StudioPage onReturn={onReturn} />);
  const user = userEvent.setup();

  await user.click(await screen.findByRole("button", { name: "← Library" }));

  expect(api.stopDraftPreview).toHaveBeenCalled();
  expect(onReturn).toHaveBeenCalledOnce();
});

it("generates another draft with the existing job as its parent", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({ status: "Ready", stage: "ready", can_preview: true, can_save: true }),
  ]);
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: "Generate another" }));

  expect(api.regenerateStudioMusic).toHaveBeenCalledWith("job_abcdefghijkl", {
    activity: "deep_work",
    genre_id: "ambient",
    sound_style_id: "ambient",
    mood_id: "focused",
    energy: "medium",
    tempo_bpm: 90,
    duration_seconds: 180,
    instrument_ids: [],
    additional_details: null,
    creative_direction: null,
  });
  expect(await screen.findByText(/Preparing your music/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Generate another" })).toBeTruthy();
});

it("requires confirmation before discarding a ready draft", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({ status: "Ready", stage: "ready", can_preview: true, can_save: true }),
  ]);
  const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);
  const discard = await screen.findByRole("button", { name: "Discard" });

  await user.click(discard);
  expect(api.discardStudioDraft).not.toHaveBeenCalled();
  await user.click(discard);

  expect(confirm).toHaveBeenCalledWith("Discard this draft?");
  expect(api.discardStudioDraft).toHaveBeenCalledWith("job_abcdefghijkl");
  await waitFor(() => expect(screen.queryByRole("button", { name: "Discard" })).toBeNull());
});

it("allows a failed creation to be discarded without preview or save", async () => {
  api.listRecentStudioJobs.mockResolvedValue([
    job({
      status: "Needs attention",
      stage: "complete",
      can_preview: false,
      can_save: false,
      can_discard: true,
    }),
  ]);
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<StudioPage onReturn={vi.fn()} />);

  fireEvent.click(await screen.findByRole("button", { name: "Discard" }));

  expect(api.discardStudioDraft).toHaveBeenCalledWith("job_abcdefghijkl");
  expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Save to My Music" })).toBeNull();
});

it("does not reopen a handled ready draft when polling returns it again", async () => {
  const ready = job({ status: "Ready", stage: "ready", can_preview: true, can_save: true });
  api.listRecentStudioJobs.mockResolvedValue([ready]);
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<StudioPage onReturn={vi.fn()} />);

  fireEvent.click(await screen.findByRole("button", { name: "Discard" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Preview" })).toBeNull());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2500);
  });

  expect(api.listRecentStudioJobs).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Discard" })).toBeNull();
  vi.useRealTimers();
});

it("shows safe creation and stored-job errors", async () => {
  api.createStudioMusic.mockRejectedValue(new Error("raw backend detail"));
  api.listRecentStudioJobs.mockResolvedValue([
    job({
      status: "Needs attention",
      stage: "complete",
      safe_message: "This music could not be created. Please try again.",
    }),
  ]);
  const user = userEvent.setup();
  render(<StudioPage onReturn={vi.fn()} />);
  expect(
    await screen.findByText(
      (_, element) =>
        element?.tagName === "P" &&
        element.textContent?.includes("This music could not be created. Please try again.") ===
          true,
    ),
  ).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Generate" }));
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Your music could not be started. Please try again.",
  );
});

it.each(["setup_required", "unsupported", "needs_attention"] as const)(
  "shows a calm %s state",
  async (state) => {
    api.getStudioCapability.mockResolvedValue({
      state,
      detail: "Music Studio is unavailable right now.",
    });
    render(<StudioPage onReturn={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Return to Library" })).toBeTruthy();
  },
);

it("shows live setup state and makes cancellation reachable", async () => {
  api.getStudioCapability.mockResolvedValue({ state: "setup_required", detail: null });
  api.startRuntimeInstall.mockResolvedValue({
    status: "installing",
    stage: "downloading",
    detail: "Downloading Music Studio file 2 of 8.",
    downloaded_bytes: 500,
    total_bytes: 1000,
    resumable: true,
  });
  api.cancelRuntimeInstall.mockResolvedValue({
    status: "installing",
    stage: "installing",
    detail: "Cancelling Music Studio setup.",
  });
  render(<StudioPage onReturn={vi.fn()} />);

  fireEvent.click(await screen.findByRole("button", { name: "Set up Music Studio" }));
  const cancel = await screen.findByRole("button", { name: "Cancel setup" });
  expect(screen.getByText("Downloading Music Studio file 2 of 8.")).toBeTruthy();
  expect(screen.getByRole("progressbar", { name: "Music Studio download progress" })).toBeTruthy();
  expect(screen.getByText("50%")).toBeTruthy();
  fireEvent.click(cancel);

  await waitFor(() => expect(api.cancelRuntimeInstall).toHaveBeenCalledOnce());
  expect(await screen.findByText("Cancelling Music Studio setup.")).toBeTruthy();
});

it("returns to the library", async () => {
  const onReturn = vi.fn();
  api.getStudioCapability.mockResolvedValue({ state: "setup_required", detail: null });
  render(<StudioPage onReturn={onReturn} />);
  await userEvent.click(await screen.findByRole("button", { name: "Return to Library" }));
  expect(onReturn).toHaveBeenCalledOnce();
});

it("shows detected hardware and minimum requirements on the setup view", async () => {
  api.getStudioCapability.mockResolvedValue({
    state: "setup_required",
    detail: "Music Studio is ready to install on this device.",
    required_bytes: 14_212_159_917,
    free_bytes: 200_000_000_000,
    hardware: {
      architecture: "x86_64",
      memory_bytes: 34_359_738_368,
      accelerator: "NVIDIA CUDA",
      vram_bytes: 12_884_901_888,
      cuda: true,
    },
    requirements: {
      architecture: "x86_64",
      min_memory_bytes: 17_179_869_184,
      min_vram_bytes: 8_589_934_592,
      cuda_required: true,
      min_free_disk_bytes: 15_317_028_915,
    },
  });
  render(<StudioPage onReturn={vi.fn()} />);

  const panel = await screen.findByRole("group", { name: "Music Studio requirements" });
  expect(panel.textContent).toContain("16 GiB RAM");
  expect(panel.textContent).toContain("8 GiB VRAM");
  expect(panel.textContent).toContain("Detected");
  expect(panel.textContent).toContain("NVIDIA CUDA");
  expect(panel.textContent).toContain("32 GiB");
  expect(panel.textContent).toContain("12 GiB");
  // The bundled-runtime reassurance copy is present.
  expect(screen.getByText(/bundles its own Python/)).toBeTruthy();
});

it("renders the unsupported state with actionable detail and no install button", async () => {
  api.getStudioCapability.mockResolvedValue({
    state: "unsupported",
    detail:
      "Music Studio is not supported on this device. System RAM is about 4 GiB; Music Studio needs at least 16 GiB.",
    hardware: {
      architecture: "x86_64",
      memory_bytes: 4_294_967_296,
      accelerator: null,
      vram_bytes: null,
      cuda: null,
    },
    requirements: {
      architecture: "x86_64",
      min_memory_bytes: 17_179_869_184,
      min_vram_bytes: 8_589_934_592,
      cuda_required: true,
      min_free_disk_bytes: 15_317_028_915,
    },
  });
  render(<StudioPage onReturn={vi.fn()} />);

  expect(await screen.findByText(/not supported on this device/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Set up Music Studio" })).toBeNull();
  expect(screen.getByRole("group", { name: "Music Studio requirements" }).textContent).toContain(
    "4 GiB",
  );
});
