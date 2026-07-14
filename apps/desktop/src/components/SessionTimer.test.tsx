import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { SessionSnapshot } from "../lib/types";
import { SessionTimer } from "./SessionTimer";

afterEach(cleanup);

const BREAK: SessionSnapshot = {
  status: "playing",
  activity: "deep_work",
  intensity: "medium",
  kind: { kind: "interval", work_seconds: 1_500, break_seconds: 300, repeats: 4 },
  phase: "break",
  current_round: 2,
  total_rounds: 4,
  focus_elapsed_seconds: 3_000,
  current_phase_remaining_seconds: 270,
  total_remaining_seconds: 3_870,
};

describe("SessionTimer", () => {
  it("shows phase, round, focus elapsed, phase remaining, and total remaining", () => {
    render(<SessionTimer snapshot={BREAK} />);
    expect(screen.getByLabelText("Break remaining").textContent).toBe("4:30");
    expect(screen.getByText("Silent break · Round 2 of 4")).toBeTruthy();
    expect(screen.getByLabelText("Elapsed focus work").textContent).toContain("50:00");
    expect(screen.getByLabelText("Total session remaining").textContent).toContain("1:04:30");
  });

  it("announces gentle completion", () => {
    render(
      <SessionTimer
        snapshot={{
          ...BREAK,
          status: "expired",
          phase: null,
          current_round: null,
          current_phase_remaining_seconds: null,
          total_remaining_seconds: 0,
        }}
      />,
    );
    expect(screen.getByText("Session complete.")).toBeTruthy();
  });
});
