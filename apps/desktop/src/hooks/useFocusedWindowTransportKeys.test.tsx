import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useFocusedWindowTransportKeys } from "./useFocusedWindowTransportKeys";
import type { SessionStatus } from "../lib/types";

interface HarnessProps {
  status: SessionStatus;
  pause?: () => Promise<void> | void;
  resume?: () => Promise<void> | void;
  stop?: () => Promise<void> | void;
  navigationAvailable?: boolean;
  next?: () => Promise<void> | void;
  previous?: () => Promise<void> | void;
  reportError?: (message: string) => void;
}

function Harness({
  status,
  pause = vi.fn(),
  resume = vi.fn(),
  stop = vi.fn(),
  navigationAvailable = false,
  next = vi.fn(),
  previous = vi.fn(),
  reportError = vi.fn(),
}: HarnessProps) {
  useFocusedWindowTransportKeys({
    status,
    pause,
    resume,
    stop,
    navigationAvailable,
    next,
    previous,
    reportError,
  });
  return <button type="button">Native control</button>;
}

async function flushKeys() {
  await act(async () => Promise.resolve());
}

afterEach(cleanup);

it("pauses playing and resumes paused sessions with Space without starting inactive sessions", async () => {
  const pause = vi.fn();
  const resume = vi.fn();
  const { rerender } = render(<Harness status="playing" pause={pause} resume={resume} />);
  fireEvent.keyDown(window, { key: " " });
  await flushKeys();
  expect(pause).toHaveBeenCalledOnce();

  rerender(<Harness status="paused" pause={pause} resume={resume} />);
  fireEvent.keyDown(window, { key: " " });
  await flushKeys();
  expect(resume).toHaveBeenCalledOnce();

  rerender(<Harness status="idle" pause={pause} resume={resume} />);
  fireEvent.keyDown(window, { key: " " });
  await flushKeys();
  expect(pause).toHaveBeenCalledOnce();
  expect(resume).toHaveBeenCalledOnce();
});

it("handles focused-window media keys, including MediaStop", async () => {
  const pause = vi.fn();
  const stop = vi.fn();
  render(<Harness status="playing" pause={pause} stop={stop} />);
  fireEvent.keyDown(window, { key: "MediaPlayPause" });
  fireEvent.keyDown(window, { key: "MediaStop" });
  await flushKeys();
  expect(pause).toHaveBeenCalledOnce();
  expect(stop).toHaveBeenCalledOnce();
});

it("handles delivered track media keys once and ignores them when navigation is unavailable", async () => {
  const next = vi.fn();
  const previous = vi.fn();
  const { rerender } = render(
    <Harness status="playing" navigationAvailable next={next} previous={previous} />,
  );
  fireEvent.keyDown(window, { key: "MediaTrackNext" });
  fireEvent.keyDown(window, { key: "MediaTrackPrevious" });
  await flushKeys();
  await flushKeys();
  expect(next).toHaveBeenCalledOnce();
  expect(previous).not.toHaveBeenCalled();

  rerender(<Harness status="playing" next={next} previous={previous} />);
  fireEvent.keyDown(window, { key: "MediaTrackPrevious" });
  await flushKeys();
  expect(previous).not.toHaveBeenCalled();
});

it("ignores repeats, modified Space, editable content, and native controls", async () => {
  const pause = vi.fn();
  const { getByRole } = render(<Harness status="playing" pause={pause} />);
  const input = document.createElement("input");
  document.body.append(input);
  fireEvent.keyDown(window, { key: " ", repeat: true });
  fireEvent.keyDown(window, { key: "MediaPlayPause", repeat: true });
  fireEvent.keyDown(window, { key: " ", ctrlKey: true });
  fireEvent.keyDown(input, { key: " " });
  fireEvent.keyDown(getByRole("button", { name: "Native control" }), { key: " " });
  await flushKeys();
  expect(pause).not.toHaveBeenCalled();
  input.remove();
});

it("cleans up its window listener on unmount", async () => {
  const pause = vi.fn();
  const { unmount } = render(<Harness status="playing" pause={pause} />);
  unmount();
  fireEvent.keyDown(window, { key: " " });
  await flushKeys();
  expect(pause).not.toHaveBeenCalled();
});

it("uses the latest status and handlers after a rerender", async () => {
  const oldPause = vi.fn();
  const resume = vi.fn();
  const { rerender } = render(<Harness status="playing" pause={oldPause} />);
  rerender(<Harness status="paused" resume={resume} />);
  fireEvent.keyDown(window, { key: "MediaPlayPause" });
  await flushKeys();
  expect(oldPause).not.toHaveBeenCalled();
  expect(resume).toHaveBeenCalledOnce();
});

it("serializes rapid actions and reports an unexpected command failure", async () => {
  let releasePause: () => void;
  const pause = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        releasePause = resolve;
      }),
  );
  const resume = vi.fn();
  const reportError = vi.fn();
  const { rerender } = render(
    <Harness status="playing" pause={pause} resume={resume} reportError={reportError} />,
  );
  fireEvent.keyDown(window, { key: "MediaPlayPause" });
  fireEvent.keyDown(window, { key: "MediaPlayPause" });
  await flushKeys();
  expect(pause).toHaveBeenCalledOnce();
  expect(resume).not.toHaveBeenCalled();
  await act(async () => releasePause!());
  expect(resume).toHaveBeenCalledOnce();

  const failed = vi.fn().mockRejectedValue(new Error("native unavailable"));
  rerender(<Harness status="playing" pause={failed} reportError={reportError} />);
  fireEvent.keyDown(window, { key: " " });
  await flushKeys();
  await flushKeys();
  expect(reportError).toHaveBeenCalledWith(expect.stringContaining("native unavailable"));
});
