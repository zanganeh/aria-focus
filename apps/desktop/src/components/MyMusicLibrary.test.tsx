import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { MyMusicItem } from "../lib/types";
import { MyMusicLibrary } from "./MyMusicLibrary";

const api = vi.hoisted(() => ({
  listMyMusic: vi.fn(),
  startMyMusic: vi.fn(),
  renameMyMusic: vi.fn(),
  deleteMyMusic: vi.fn(),
}));

vi.mock("../lib/api", () => api);

const ITEM: MyMusicItem = {
  item_id: "generated.local.job_abcdefghijkl.item",
  title: "Calm momentum",
  duration_seconds: 180,
  created_at: 1_786_000_000,
  activity: "deep_work",
  job_id: "job_abcdefghijkl",
};

beforeEach(() => {
  vi.clearAllMocks();
  api.listMyMusic.mockResolvedValue([ITEM]);
  api.startMyMusic.mockResolvedValue(undefined);
  api.renameMyMusic.mockResolvedValue(undefined);
  api.deleteMyMusic.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("lists created-on-device music and plays the selected item", async () => {
  const user = userEvent.setup();
  const onStarted = vi.fn().mockResolvedValue(undefined);
  render(<MyMusicLibrary disabled={false} onError={vi.fn()} onStarted={onStarted} />);

  expect(await screen.findByText("Calm momentum")).toBeTruthy();
  expect(screen.getByText("Created on this device · 180 sec")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Play" }));

  expect(api.listMyMusic).toHaveBeenCalledOnce();
  expect(api.startMyMusic).toHaveBeenCalledWith(ITEM.item_id, "deep_work");
  expect(onStarted).toHaveBeenCalledOnce();
});

it("renames an item and updates its displayed customer title", async () => {
  vi.spyOn(window, "prompt").mockReturnValue("  Steady rain  ");
  const user = userEvent.setup();
  render(<MyMusicLibrary disabled={false} onError={vi.fn()} onStarted={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: "Rename" }));

  expect(api.renameMyMusic).toHaveBeenCalledWith(ITEM.item_id, "  Steady rain  ");
  expect(await screen.findByText("Steady rain")).toBeTruthy();
});

it("requires delete confirmation and removes only after the backend succeeds", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
  const onCatalogueChange = vi.fn();
  const user = userEvent.setup();
  render(
    <MyMusicLibrary
      disabled={false}
      onError={vi.fn()}
      onStarted={vi.fn()}
      onCatalogueChange={onCatalogueChange}
    />,
  );
  const remove = await screen.findByRole("button", { name: "Delete" });

  await user.click(remove);
  expect(api.deleteMyMusic).not.toHaveBeenCalled();
  await user.click(remove);

  expect(confirm).toHaveBeenCalledWith("Delete Calm momentum?");
  expect(api.deleteMyMusic).toHaveBeenCalledWith(ITEM.item_id);
  await waitFor(() => expect(screen.queryByText("Calm momentum")).toBeNull());
  expect(onCatalogueChange).toHaveBeenCalledOnce();
});

it("shows only safe customer errors for rejected play, rename, and delete commands", async () => {
  api.startMyMusic.mockRejectedValue(new Error("raw playback detail"));
  api.renameMyMusic.mockRejectedValue(new Error("raw database detail"));
  api.deleteMyMusic.mockRejectedValue(new Error("raw path detail"));
  vi.spyOn(window, "prompt").mockReturnValue("New title");
  vi.spyOn(window, "confirm").mockReturnValue(true);
  const onError = vi.fn();
  const user = userEvent.setup();
  render(<MyMusicLibrary disabled={false} onError={onError} onStarted={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: "Play" }));
  await user.click(screen.getByRole("button", { name: "Rename" }));
  await user.click(screen.getByRole("button", { name: "Delete" }));

  await waitFor(() => expect(onError).toHaveBeenCalledTimes(3));
  expect(onError.mock.calls.map(([message]) => message)).toEqual([
    "This music could not be played right now.",
    "That name could not be saved.",
    "Stop playback before deleting this music.",
  ]);
  expect(onError.mock.calls.flat().join(" ")).not.toContain("raw");
});

it("renders no section for an empty library", async () => {
  api.listMyMusic.mockResolvedValue([]);
  render(<MyMusicLibrary disabled={false} onError={vi.fn()} onStarted={vi.fn()} />);

  await waitFor(() => expect(api.listMyMusic).toHaveBeenCalledOnce());
  expect(screen.queryByRole("region", { name: "My Music" })).toBeNull();
});

it("reports a safe error when My Music cannot be loaded", async () => {
  api.listMyMusic.mockRejectedValue(new Error("raw database detail"));
  const onError = vi.fn();
  render(<MyMusicLibrary disabled={false} onError={onError} onStarted={vi.fn()} />);

  await waitFor(() =>
    expect(onError).toHaveBeenCalledWith("My Music could not be loaded right now."),
  );
  expect(onError.mock.calls.flat().join(" ")).not.toContain("raw");
});

it("disables every item action when library playback is unavailable", async () => {
  render(<MyMusicLibrary disabled onError={vi.fn()} onStarted={vi.fn()} />);

  expect((await screen.findByRole("button", { name: "Play" })).hasAttribute("disabled")).toBe(true);
  expect(screen.getByRole("button", { name: "Rename" }).hasAttribute("disabled")).toBe(true);
  expect(screen.getByRole("button", { name: "Delete" }).hasAttribute("disabled")).toBe(true);
});
