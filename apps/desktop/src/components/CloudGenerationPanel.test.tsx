import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { CloudGenerationPanel } from "./CloudGenerationPanel";
import type { CloudModel } from "../lib/types";
import {
  activateCloudGeneration,
  createCloudGeneration,
  estimateCloudGeneration,
  getActiveCloudGeneration,
  getCloudKeyStatus,
  getCloudGeneration,
  getCloudGenerationItems,
  listCloudModels,
  saveCloudKey,
  startCloudGenerationPreview,
} from "../lib/api";

vi.mock("../lib/api", () => ({
  getCloudKeyStatus: vi.fn(),
  getActiveCloudGeneration: vi.fn(),
  saveCloudKey: vi.fn(),
  removeCloudKey: vi.fn(),
  listCloudModels: vi.fn(),
  estimateCloudGeneration: vi.fn(),
  createCloudGeneration: vi.fn(),
  cancelCloudGeneration: vi.fn(),
  getCloudGeneration: vi.fn(),
  getCloudGenerationItems: vi.fn(),
  activateCloudGeneration: vi.fn(),
  startCloudGenerationPreview: vi.fn(),
  stopDraftPreview: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(getActiveCloudGeneration).mockResolvedValue(null);
  vi.mocked(getCloudKeyStatus).mockResolvedValue({ configured: false, masked_suffix: null });
  vi.mocked(saveCloudKey).mockResolvedValue({ configured: true, masked_suffix: "1234" });
  const pricedModels: CloudModel[] = [
    {
      id: "google/lyria-3-pro-preview",
      name: "Lyria 3 Pro Preview",
      description: null,
      input_modalities: ["text"],
      output_modalities: ["audio"],
      supported_parameters: [],
      pricing: { prompt: "0", completion: "0", request: "0.08", image: null },
      context_length: null,
      curated: true,
    },
    {
      id: "google/gemini-2.5-flash",
      name: "Gemini 2.5 Flash",
      description: null,
      input_modalities: ["text"],
      output_modalities: ["text"],
      supported_parameters: [],
      pricing: { prompt: "0.0000003", completion: "0.0000025", request: null, image: null },
      context_length: null,
      curated: true,
    },
    {
      id: "google/gemini-2.5-flash-image",
      name: "Gemini 2.5 Flash Image",
      description: null,
      input_modalities: ["text"],
      output_modalities: ["image"],
      supported_parameters: [],
      pricing: { prompt: "0.0000003", completion: "0.0000025", request: null, image: "0.00003" },
      context_length: null,
      curated: true,
    },
  ];
  vi.mocked(listCloudModels).mockResolvedValue(pricedModels);
  vi.mocked(estimateCloudGeneration).mockResolvedValue({
    target_count: 1,
    audio_microdollars: 80_000,
    text_microdollars: 2_000,
    image_microdollars: 5_000,
    total_microdollars: 87_000,
    currency: "USD",
    pricing_source: "OpenRouter model pricing + published media rates",
  });
  vi.mocked(createCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_test",
    state: "running",
    target_count: 1,
    completed_count: 0,
    failed_count: 0,
    reserved_microdollars: 0,
    actual_microdollars: 0,
    budget_microdollars: 100_000,
    activation_version: null,
    error_code: null,
    error_message: null,
  });
});

afterEach(() => cleanup());

it("explains online generation and stores the key only after explicit validation", async () => {
  const onOpenSettings = vi.fn();
  render(<CloudGenerationPanel onOpenSettings={onOpenSettings} />);
  expect(screen.getByText(/Create music/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Set up OpenRouter in Settings" }));
  expect(onOpenSettings).toHaveBeenCalledOnce();
  cleanup();
  render(<CloudGenerationPanel view="settings" />);
  const input = screen.getByPlaceholderText("Paste a temporary key");
  fireEvent.change(input, { target: { value: "a-valid-looking-openrouter-key" } });
  fireEvent.click(screen.getByRole("button", { name: "Validate and save key" }));
  await waitFor(() => expect(saveCloudKey).toHaveBeenCalledWith("a-valid-looking-openrouter-key"));
  expect(screen.getByText(/stored in your operating-system credential store/)).toBeTruthy();
});

it("requires a budget confirmation before starting a paid batch", async () => {
  vi.mocked(getCloudKeyStatus).mockResolvedValue({ configured: true, masked_suffix: "1234" });
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  render(<CloudGenerationPanel />);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Generate 1 track" })).toBeTruthy(),
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate 1 track" }));
  expect(createCloudGeneration).not.toHaveBeenCalled();
  confirm.mockRestore();
});

it("rehydrates an active batch after returning to Create", async () => {
  vi.mocked(getCloudKeyStatus).mockResolvedValue({ configured: true, masked_suffix: "1234" });
  vi.mocked(getActiveCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_running",
    state: "running",
    target_count: 5,
    completed_count: 2,
    failed_count: 0,
    reserved_microdollars: 410_000,
    actual_microdollars: 160_000,
    budget_microdollars: 500_000,
    activation_version: null,
    error_code: null,
    error_message: null,
  });
  render(<CloudGenerationPanel />);
  await waitFor(() => expect(screen.getByText("Generation running")).toBeTruthy());
  expect(screen.getByText("2/5 complete")).toBeTruthy();
});

it("shows the active batch in Settings and offers a return to Create", async () => {
  vi.mocked(getActiveCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_settings",
    state: "validated",
    target_count: 1,
    completed_count: 1,
    failed_count: 0,
    reserved_microdollars: 80_000,
    actual_microdollars: 80_000,
    budget_microdollars: 100_000,
    activation_version: null,
    error_code: null,
    error_message: null,
  });
  const onOpenCreate = vi.fn();
  render(<CloudGenerationPanel view="settings" onOpenCreate={onOpenCreate} />);
  await waitFor(() => expect(screen.getByText("Tracks ready to review")).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "Continue in Create" }));
  expect(onOpenCreate).toHaveBeenCalledOnce();
});

it("previews validated candidates and only activates after explicit save", async () => {
  vi.mocked(getCloudKeyStatus).mockResolvedValue({ configured: true, masked_suffix: "1234" });
  vi.mocked(createCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_validated",
    state: "validated",
    target_count: 1,
    completed_count: 1,
    failed_count: 0,
    reserved_microdollars: 87_000,
    actual_microdollars: 82_000,
    budget_microdollars: 100_000,
    activation_version: null,
    error_code: null,
    error_message: null,
  });
  vi.mocked(getCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_validated",
    state: "validated",
    target_count: 1,
    completed_count: 1,
    failed_count: 0,
    reserved_microdollars: 87_000,
    actual_microdollars: 82_000,
    budget_microdollars: 100_000,
    activation_version: null,
    error_code: null,
    error_message: null,
  });
  vi.mocked(getCloudGenerationItems).mockResolvedValue([
    {
      item_id: "cloud_item_1",
      ordinal: 1,
      activity: "motivation",
      state: "validated",
      audio_path: "candidate.mp3",
      cover_path: null,
      cover_art: null,
      prompt_json: "{}",
      refined_prompt: null,
      audio_sha256: "abc",
      estimated_microdollars: 80_000,
      actual_microdollars: 80_000,
      validation_json: null,
      error_code: null,
      error_message: null,
    },
  ]);
  vi.mocked(activateCloudGeneration).mockResolvedValue({
    batch_id: "cloud_batch_validated",
    state: "activated",
    target_count: 1,
    completed_count: 1,
    failed_count: 0,
    reserved_microdollars: 87_000,
    actual_microdollars: 82_000,
    budget_microdollars: 100_000,
    activation_version: "cloud_v1",
    error_code: null,
    error_message: null,
  });
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  render(<CloudGenerationPanel />);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Generate 1 track" })).toBeTruthy(),
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate 1 track" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Preview" })).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "Preview" }));
  await waitFor(() =>
    expect(startCloudGenerationPreview).toHaveBeenCalledWith(
      "cloud_batch_validated",
      "cloud_item_1",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Save and activate tracks" }));
  await waitFor(() =>
    expect(activateCloudGeneration).toHaveBeenCalledWith("cloud_batch_validated"),
  );
  expect(confirm).toHaveBeenCalledTimes(2);
  confirm.mockRestore();
});
