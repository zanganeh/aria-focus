import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { SessionEndCard } from "./SessionEndCard";

const session = {
  id: "a".repeat(32),
  activity: "deep_work" as const,
  intensity: "medium" as const,
  session_type: { kind: "countdown", seconds: 60 } as const,
  started_at: 1,
  ended_at: 61,
  end_reason: "stopped" as const,
  focus_seconds: 60,
  focus_outcome: "neutral" as const,
  sound_enjoyment: "liked" as const,
};

afterEach(cleanup);

it("clears either rating independently and saves the resulting pair", async () => {
  const onSave = vi.fn().mockResolvedValue(undefined);
  render(<SessionEndCard session={session} onSave={onSave} onSkip={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Clear focus outcome" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(onSave).toHaveBeenCalledWith(null, "liked");
});

it("lets Skip dismiss without writing ratings", () => {
  const onSave = vi.fn();
  const onSkip = vi.fn();
  render(<SessionEndCard session={session} onSave={onSave} onSkip={onSkip} />);
  fireEvent.click(screen.getByRole("button", { name: "Skip" }));
  expect(onSkip).toHaveBeenCalledOnce();
  expect(onSave).not.toHaveBeenCalled();
});
