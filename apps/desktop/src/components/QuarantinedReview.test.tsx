import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { QuarantinedReview } from "./QuarantinedReview";

const candidates = [
  {
    alias: "E",
    title: "Track E",
    review_id: "E",
    bytes: 1,
    codec: "FLAC",
    sample_rate_hz: 1,
    channels: 2,
    duration_seconds: 90,
    quarantine_status: "review",
  },
];

const roundOneAliases = ["I", "J", "N", "O", "Q", "R", "U", "X"];
const allReviewCandidates = [
  "E",
  "F",
  ...roundOneAliases,
  "K",
  "L",
  "M",
  "P",
  "S",
  "T",
  "V",
  "W",
].map((alias) => ({ ...candidates[0], alias, title: `Track ${alias}`, review_id: alias }));

beforeEach(() => {
  window.localStorage.clear();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn() },
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("bounds notes and copies the selectable JSON summary when clipboard succeeds", async () => {
  const writeText = vi.mocked(navigator.clipboard.writeText).mockResolvedValue();
  render(
    <QuarantinedReview candidates={candidates} active={false} disabled={false} onStart={vi.fn()} />,
  );
  fireEvent.click(screen.getByRole("radio", { name: "good" }));
  const note = screen.getByRole("textbox", { name: /Short note/ });
  fireEvent.change(note, { target: { value: "x".repeat(501) } });
  expect((note as HTMLTextAreaElement).value).toBe("x".repeat(500));
  fireEvent.click(screen.getByRole("button", { name: /Copy blind-triage JSON summary/ }));
  await vi.waitFor(() => expect(writeText).toHaveBeenCalledOnce());
  await vi.waitFor(() => expect(screen.getByText(/Copied review JSON/)).toBeTruthy());
  expect(screen.getByRole("textbox", { name: "Selectable blind-triage JSON" })).toBeTruthy();
});

it("keeps JSON visible with a clear message when copying fails", async () => {
  vi.mocked(navigator.clipboard.writeText).mockRejectedValue(new Error("denied"));
  render(
    <QuarantinedReview candidates={candidates} active={false} disabled={false} onStart={vi.fn()} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Copy blind-triage JSON summary/ }));
  await vi.waitFor(() => expect(screen.getByText(/Could not copy automatically/)).toBeTruthy());
  expect(screen.getByRole("textbox", { name: "Selectable blind-triage JSON" })).toBeTruthy();
});

it("shows the eight-track round by default and keeps held-back tracks available", () => {
  render(
    <QuarantinedReview
      candidates={allReviewCandidates}
      active={false}
      disabled={false}
      onStart={vi.fn()}
    />,
  );

  expect(screen.getAllByRole("listitem")).toHaveLength(8);
  expect(screen.queryByRole("button", { name: "Start quarantined review Track E" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Show held-back tracks" }));

  expect(screen.getAllByRole("listitem")).toHaveLength(allReviewCandidates.length);
  expect(screen.getByRole("button", { name: "Start quarantined review Track E" })).toBeTruthy();
});
