import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { FocusView } from "./FocusView";
import type { SessionSnapshot } from "../lib/types";

const playingInfinite: SessionSnapshot = {
  status: "playing",
  activity: "deep_work",
  intensity: "medium",
  kind: { kind: "infinite" },
  phase: "work",
  current_round: null,
  total_rounds: null,
  focus_elapsed_seconds: 125,
  current_phase_remaining_seconds: null,
  total_remaining_seconds: null,
};

function renderFocus(snapshot = playingInfinite) {
  const onPause = vi.fn();
  const onResume = vi.fn();
  const onExit = vi.fn();
  render(
    <FocusView
      snapshot={snapshot}
      activityLabel="Deep Work"
      onPause={onPause}
      onResume={onResume}
      onExit={onExit}
    />,
  );
  return { onPause, onResume, onExit };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("renders a minimal infinite-session surface and focuses Pause", () => {
  renderFocus();

  expect(screen.getByRole("main", { name: "Focus view" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Deep Work" })).toBeTruthy();
  expect(screen.getByLabelText("Focused: 2:05")).toBeTruthy();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Pause" }));
  expect(screen.getByRole("button", { name: "Exit focus view" })).toBeTruthy();
  expect(screen.queryByText(/genre|mood|volume|track|settings/i)).toBeNull();
});

it("uses remaining-time labels for countdown and interval sessions", () => {
  const { rerender } = render(
    <FocusView
      snapshot={{
        ...playingInfinite,
        kind: { kind: "countdown", seconds: 1500 },
        phase: "work",
        current_phase_remaining_seconds: 1200,
        total_remaining_seconds: 1200,
      }}
      activityLabel="Deep Work"
      onPause={vi.fn()}
      onResume={vi.fn()}
      onExit={vi.fn()}
    />,
  );
  expect(screen.getByLabelText("Work remaining: 20:00")).toBeTruthy();

  rerender(
    <FocusView
      snapshot={{
        ...playingInfinite,
        kind: { kind: "interval", work_seconds: 1500, break_seconds: 300, repeats: 4 },
        phase: "break",
        current_phase_remaining_seconds: 240,
        total_remaining_seconds: 3240,
      }}
      activityLabel="Deep Work"
      onPause={vi.fn()}
      onResume={vi.fn()}
      onExit={vi.fn()}
    />,
  );
  expect(screen.getByText("Break")).toBeTruthy();
  expect(screen.getByLabelText("Break remaining: 4:00")).toBeTruthy();
});

it("pauses or resumes with native buttons and Escape exits without changing playback", () => {
  const { onPause, onResume, onExit } = renderFocus();
  fireEvent.click(screen.getByRole("button", { name: "Pause" }));
  fireEvent.keyDown(screen.getByRole("button", { name: "Exit focus view" }), { key: "Escape" });
  expect(onPause).toHaveBeenCalledOnce();
  expect(onResume).not.toHaveBeenCalled();
  expect(onExit).toHaveBeenCalledOnce();

  cleanup();
  const paused = renderFocus({ ...playingInfinite, status: "paused" });
  fireEvent.click(screen.getByRole("button", { name: "Resume" }));
  expect(paused.onResume).toHaveBeenCalledOnce();
});
