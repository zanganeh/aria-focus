import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { TransportControls } from "./TransportControls";

afterEach(cleanup);

it("disables Start for unavailable startup services without showing a false loading state", async () => {
  const onStart = vi.fn();
  render(
    <TransportControls
      status="idle"
      starting={false}
      activityLabel="Deep work"
      startDisabled
      onStart={onStart}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  const start = screen.getByRole("button", { name: "Start Deep work focus session" });
  expect(start.hasAttribute("disabled")).toBe(true);
  expect(start.textContent).toBe("Start Deep work");
  await userEvent.setup().click(start);
  expect(onStart).not.toHaveBeenCalled();
});

it("does not render the media row before a session is active", () => {
  render(
    <TransportControls
      status="idle"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  expect(screen.getByRole("button", { name: "Start Deep work focus session" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Pause session" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Resume session" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Stop session" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Previous installed track" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Next installed track" })).toBeNull();
});

it("keeps active transport actions available when only packs are unavailable", () => {
  render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      startDisabled
      actionsDisabled={false}
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  expect(screen.getByRole("button", { name: "Pause session" }).hasAttribute("disabled")).toBe(
    false,
  );
  expect(screen.getByRole("button", { name: "Stop session" }).hasAttribute("disabled")).toBe(false);
});

it("calls pause and resume from the central media control", async () => {
  const onPause = vi.fn();
  const onResume = vi.fn();
  const { rerender } = render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={onPause}
      onResume={onResume}
      onStop={vi.fn()}
    />,
  );

  await userEvent.setup().click(screen.getByRole("button", { name: "Pause session" }));
  expect(onPause).toHaveBeenCalledOnce();
  expect(onResume).not.toHaveBeenCalled();

  rerender(
    <TransportControls
      status="paused"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={onPause}
      onResume={onResume}
      onStop={vi.fn()}
    />,
  );
  await userEvent.setup().click(screen.getByRole("button", { name: "Resume session" }));
  expect(onResume).toHaveBeenCalledOnce();
});

it("calls stop and disables stop plus pause when core is unavailable", async () => {
  const onStop = vi.fn();
  const { rerender } = render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={onStop}
    />,
  );

  await userEvent.setup().click(screen.getByRole("button", { name: "Stop session" }));
  expect(onStop).toHaveBeenCalledOnce();

  rerender(
    <TransportControls
      status="paused"
      starting={false}
      activityLabel="Deep work"
      actionsDisabled
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={onStop}
    />,
  );
  expect(screen.getByRole("button", { name: "Resume session" }).hasAttribute("disabled")).toBe(
    true,
  );
  expect(screen.getByRole("button", { name: "Stop session" }).hasAttribute("disabled")).toBe(true);
  await userEvent.setup().click(screen.getByRole("button", { name: "Stop session" }));
  expect(onStop).toHaveBeenCalledOnce();
});

it("always shows navigation buttons while active but disables them when unavailable", async () => {
  const onNext = vi.fn();
  const onPrevious = vi.fn();
  const { rerender } = render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
      onNext={onNext}
      onPrevious={onPrevious}
    />,
  );

  const previous = screen.getByRole("button", { name: "Previous installed track" });
  const next = screen.getByRole("button", { name: "Next installed track" });
  // Navigation is unavailable by default: visible but disabled, no handler calls.
  expect(previous.hasAttribute("disabled")).toBe(true);
  expect(next.hasAttribute("disabled")).toBe(true);
  await userEvent.setup().click(previous);
  await userEvent.setup().click(next);
  expect(onPrevious).not.toHaveBeenCalled();
  expect(onNext).not.toHaveBeenCalled();

  rerender(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      navigationAvailable
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
      onNext={onNext}
      onPrevious={onPrevious}
    />,
  );
  const enabledPrevious = screen.getByRole("button", { name: "Previous installed track" });
  const enabledNext = screen.getByRole("button", { name: "Next installed track" });
  expect(enabledPrevious.hasAttribute("disabled")).toBe(false);
  expect(enabledNext.hasAttribute("disabled")).toBe(false);
  await userEvent.setup().click(enabledPrevious);
  await userEvent.setup().click(enabledNext);
  expect(onPrevious).toHaveBeenCalledOnce();
  expect(onNext).toHaveBeenCalledOnce();
});

it("disables navigation while pending or when actions are disabled", () => {
  const { rerender } = render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      navigationAvailable
      navigationPending
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
      onNext={vi.fn()}
      onPrevious={vi.fn()}
    />,
  );
  expect(
    screen.getByRole("button", { name: "Next installed track" }).hasAttribute("disabled"),
  ).toBe(true);
  expect(
    screen.getByRole("button", { name: "Previous installed track" }).hasAttribute("disabled"),
  ).toBe(true);

  rerender(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      navigationAvailable
      actionsDisabled
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
      onNext={vi.fn()}
      onPrevious={vi.fn()}
    />,
  );
  expect(
    screen.getByRole("button", { name: "Next installed track" }).hasAttribute("disabled"),
  ).toBe(true);
  expect(
    screen.getByRole("button", { name: "Previous installed track" }).hasAttribute("disabled"),
  ).toBe(true);
  expect(screen.getByRole("button", { name: "Pause session" }).hasAttribute("disabled")).toBe(true);
});

it("does not render the shortcut sentence", () => {
  render(
    <TransportControls
      status="playing"
      starting={false}
      activityLabel="Deep work"
      onStart={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onStop={vi.fn()}
    />,
  );
  expect(screen.queryByText(/Space: pause\/resume/)).toBeNull();
  expect(screen.queryByRole("group", { name: "Track navigation" })).toBeNull();
});
