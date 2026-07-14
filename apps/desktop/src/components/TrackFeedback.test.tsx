import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getItemFeedback, setItemFeedback } from "../lib/api";
import { TrackFeedback } from "./TrackFeedback";

vi.mock("../lib/api", () => ({ getItemFeedback: vi.fn(), setItemFeedback: vi.fn() }));

const source = (item_id: string) => ({
  pack_id: "pack",
  pack_title: "Pack",
  item_id,
  item_title: item_id,
  variant_id: "base",
  fallback: false,
});
const feedback = (
  item_id: string,
  focus_feedback: "helps_focus" | "neutral" | "distracting" | null = null,
  enjoyment: "liked" | "not_for_me" | null = null,
) => ({ item_id, activity: "deep_work" as const, focus_feedback, enjoyment });

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.mocked(getItemFeedback).mockReset();
  vi.mocked(setItemFeedback).mockReset();
});

it("keeps neutral distinct from clearing a focus answer", async () => {
  vi.mocked(getItemFeedback).mockResolvedValue(feedback("track", "neutral", "liked"));
  vi.mocked(setItemFeedback).mockResolvedValue(feedback("track", null, "liked"));
  render(<TrackFeedback source={source("track")} activity="deep_work" onError={vi.fn()} />);
  expect(
    ((await screen.findByRole("radio", { name: "Neutral" })) as HTMLInputElement).checked,
  ).toBe(true);
  fireEvent.click(screen.getAllByRole("button", { name: "Clear" })[0]);
  await waitFor(() => expect(setItemFeedback).toHaveBeenCalledWith("track", null, "liked"));
});
afterEach(cleanup);

it("shows independent focus and sound questions and preserves the other axis on save", async () => {
  vi.mocked(getItemFeedback).mockResolvedValue(feedback("track", "helps_focus", "liked"));
  vi.mocked(setItemFeedback).mockResolvedValue(feedback("track", "helps_focus", "not_for_me"));
  render(<TrackFeedback source={source("track")} activity="deep_work" onError={vi.fn()} />);
  expect(
    ((await screen.findByRole("radio", { name: "Helps focus" })) as HTMLInputElement).checked,
  ).toBe(true);
  expect((screen.getByRole("radio", { name: "Liked" }) as HTMLInputElement).checked).toBe(true);
  fireEvent.click(screen.getByRole("radio", { name: "Not for me" }));
  await waitFor(() =>
    expect(setItemFeedback).toHaveBeenCalledWith("track", "helps_focus", "not_for_me"),
  );
});

it("ignores a delayed save for an item that is no longer displayed", async () => {
  const save = deferred<ReturnType<typeof feedback>>();
  vi.mocked(getItemFeedback).mockImplementation(async (itemId) => feedback(itemId));
  vi.mocked(setItemFeedback).mockReturnValueOnce(save.promise);
  const view = render(
    <TrackFeedback source={source("a")} activity="deep_work" onError={vi.fn()} />,
  );
  await screen.findByRole("radio", { name: "Liked" });
  fireEvent.click(screen.getByRole("radio", { name: "Helps focus" }));
  view.rerender(<TrackFeedback source={source("b")} activity="deep_work" onError={vi.fn()} />);
  save.resolve(feedback("a", "helps_focus"));
  await waitFor(() =>
    expect((screen.getByRole("radio", { name: "Helps focus" }) as HTMLInputElement).checked).toBe(
      false,
    ),
  );
});

it("reports failed saves without optimistically changing either answer", async () => {
  const onError = vi.fn();
  vi.mocked(getItemFeedback).mockResolvedValue(feedback("track"));
  vi.mocked(setItemFeedback).mockRejectedValue(new Error("offline"));
  render(<TrackFeedback source={source("track")} activity="deep_work" onError={onError} />);
  await screen.findByRole("radio", { name: "Liked" });
  fireEvent.click(screen.getByRole("radio", { name: "Liked" }));
  await waitFor(() => expect(onError).toHaveBeenCalled());
  expect((screen.getByRole("radio", { name: "Liked" }) as HTMLInputElement).checked).toBe(false);
});
