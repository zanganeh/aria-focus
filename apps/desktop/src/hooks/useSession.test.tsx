import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBanner } from "../components/ErrorBanner";
import {
  getSnapshot,
  getMasterVolume,
  pauseSession,
  resumeSession,
  setActivity,
  setIntensity,
  setMasterVolume,
  setSessionType,
  startSession,
  stopSession,
} from "../lib/api";
import type { SessionSnapshot } from "../lib/types";
import { useSession } from "./useSession";

vi.mock("../lib/api", () => ({
  getSnapshot: vi.fn(),
  getMasterVolume: vi.fn(),
  pauseSession: vi.fn(),
  resumeSession: vi.fn(),
  setActivity: vi.fn(),
  setIntensity: vi.fn(),
  setMasterVolume: vi.fn(),
  setSessionType: vi.fn(),
  startSession: vi.fn(),
  stopSession: vi.fn(),
}));

const IDLE_SNAPSHOT: SessionSnapshot = {
  status: "idle",
  activity: "deep_work",
  intensity: "medium",
  kind: { kind: "infinite" },
  phase: null,
  current_round: null,
  total_rounds: null,
  focus_elapsed_seconds: 0,
  current_phase_remaining_seconds: null,
  total_remaining_seconds: null,
};

function Harness() {
  const session = useSession();
  return (
    <>
      <button type="button" onClick={() => void session.start()}>
        Start
      </button>
      <button type="button" onClick={() => void session.adoptStartedSession()}>
        Adopt playback
      </button>
      <button type="button" onClick={() => void session.changeActivity("learning")}>
        Learning
      </button>
      <button
        type="button"
        onClick={() => void session.changeSessionType({ kind: "countdown", seconds: 1_500 })}
      >
        Countdown
      </button>
      <ErrorBanner message={session.error} onDismiss={session.dismissError} />
    </>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getSnapshot).mockResolvedValue(IDLE_SNAPSHOT);
  vi.mocked(getMasterVolume).mockResolvedValue(70);
  vi.mocked(pauseSession).mockResolvedValue();
  vi.mocked(resumeSession).mockResolvedValue();
  vi.mocked(setActivity).mockResolvedValue();
  vi.mocked(setIntensity).mockResolvedValue();
  vi.mocked(setMasterVolume).mockResolvedValue(70);
  vi.mocked(setSessionType).mockResolvedValue();
  vi.mocked(startSession).mockResolvedValue();
  vi.mocked(stopSession).mockResolvedValue();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useSession command errors", () => {
  it("refreshes and polls a session started by another local command", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<Harness />);
    await waitFor(() => expect(getSnapshot).toHaveBeenCalledOnce());

    await userEvent.click(screen.getByRole("button", { name: "Adopt playback" }));
    await waitFor(() => expect(getSnapshot).toHaveBeenCalledTimes(2));
    await vi.advanceTimersByTimeAsync(250);

    expect(getSnapshot).toHaveBeenCalledTimes(3);
    expect(startSession).not.toHaveBeenCalled();
  });

  it("handles a rejected native start command and exposes it visibly", async () => {
    vi.mocked(startSession).mockRejectedValue("no default audio output device is available");
    const user = userEvent.setup();
    render(<Harness />);
    await waitFor(() => expect(getSnapshot).toHaveBeenCalled());

    await user.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain(
        "Unable to start the session: no default audio output device is available",
      );
    });
  });

  it("handles a rejected activity command without an unhandled promise", async () => {
    vi.mocked(setActivity).mockRejectedValue("stop the active session before changing activity");
    const user = userEvent.setup();
    render(<Harness />);
    await waitFor(() => expect(getSnapshot).toHaveBeenCalled());

    await user.click(screen.getByRole("button", { name: "Learning" }));
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain(
        "Unable to change activity: stop the active session before changing activity",
      );
    });
  });

  it("handles timer validation or persistence failures through the existing error path", async () => {
    vi.mocked(setSessionType).mockRejectedValue("stored timer configuration is invalid");
    const user = userEvent.setup();
    render(<Harness />);
    await waitFor(() => expect(getSnapshot).toHaveBeenCalled());

    await user.click(screen.getByRole("button", { name: "Countdown" }));
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain(
        "Unable to change session timer: stored timer configuration is invalid",
      );
    });
  });
});
