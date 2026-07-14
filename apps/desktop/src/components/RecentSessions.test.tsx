import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { RecentSessions } from "./RecentSessions";

afterEach(cleanup);

it("stays absent when empty and shows truthful finalized session details when present", () => {
  const { rerender } = render(<RecentSessions sessions={[]} />);
  expect(screen.queryByText("Recent sessions")).toBeNull();

  rerender(
    <RecentSessions
      sessions={[
        {
          id: "0123456789abcdef0123456789abcdef",
          activity: "deep_work",
          intensity: "medium",
          session_type: { kind: "countdown", seconds: 1800 },
          started_at: 100,
          ended_at: 160,
          end_reason: "expired",
          focus_seconds: 61,
          focus_outcome: "helped_focus",
          sound_enjoyment: "liked",
        },
      ]}
    />,
  );
  expect(screen.getByText("Recent sessions")).toBeTruthy();
  expect(screen.getByRole("list", { name: "Recent sessions" }).textContent).toMatch(
    /deep work.*1m 1s.*expired.*helped_focus.*liked/,
  );
});
