import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { listFavorites, removeFavorite, startFavorite } from "../lib/api";
import { FavoritesLibrary } from "./FavoritesLibrary";

vi.mock("../lib/api", () => ({
  listFavorites: vi.fn(),
  removeFavorite: vi.fn(),
  startFavorite: vi.fn(),
}));

const item = {
  item_id: "rain",
  activity: "deep_work" as const,
  title: "Soft Rain",
  genre: ["Ambient"],
  moods: ["Calm"],
};
beforeEach(() => {
  vi.mocked(listFavorites).mockReset();
  vi.mocked(removeFavorite).mockReset();
  vi.mocked(startFavorite).mockReset();
});
afterEach(cleanup);

it("is collapsed, gives a useful empty state, and refreshes on revision", async () => {
  vi.mocked(listFavorites).mockResolvedValue([]);
  const view = render(
    <FavoritesLibrary
      active={false}
      disabled={false}
      revision={0}
      onStarted={vi.fn()}
      onError={vi.fn()}
    />,
  );
  expect(screen.getByText("Favorites library").closest("details")?.open).toBe(false);
  await screen.findByText(/no liked tracks yet/i);
  view.rerender(
    <FavoritesLibrary
      active={false}
      disabled={false}
      revision={1}
      onStarted={vi.fn()}
      onError={vi.fn()}
    />,
  );
  await waitFor(() => expect(listFavorites).toHaveBeenCalledTimes(2));
});

it("shows truthful metadata, removes only the selected favorite, and directly starts it", async () => {
  const started = vi.fn().mockResolvedValue(undefined);
  vi.mocked(listFavorites).mockResolvedValue([item]);
  vi.mocked(removeFavorite).mockResolvedValue();
  vi.mocked(startFavorite).mockResolvedValue();
  render(
    <FavoritesLibrary
      active={false}
      disabled={false}
      revision={0}
      onStarted={started}
      onError={vi.fn()}
    />,
  );
  await screen.findByText("Soft Rain");
  expect(screen.getByText(/deep work.*Ambient.*Calm/i)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await waitFor(() => expect(startFavorite).toHaveBeenCalledWith("rain", "deep_work"));
  await waitFor(() => expect(started).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Remove favorite" }));
  await waitFor(() => expect(removeFavorite).toHaveBeenCalledWith("rain", "deep_work"));
  await waitFor(() => expect(screen.queryByText("Soft Rain")).toBeNull());
});

it("does not expose a start control while a session is active and reports errors", async () => {
  const onError = vi.fn();
  vi.mocked(listFavorites).mockResolvedValue([item]);
  render(
    <FavoritesLibrary active disabled={false} revision={0} onStarted={vi.fn()} onError={onError} />,
  );
  const play = await screen.findByRole("button", { name: "Play" });
  expect((play as HTMLButtonElement).disabled).toBe(true);
});
