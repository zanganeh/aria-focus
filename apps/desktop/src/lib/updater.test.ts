import { beforeEach, expect, it, vi } from "vitest";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { findAvailableUpdate, installAndRelaunch } from "./updater";
import type { Update } from "@tauri-apps/plugin-updater";

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(relaunch).mockResolvedValue();
});

it("treats no update as a normal result", async () => {
  vi.mocked(check).mockResolvedValue(null);

  await expect(findAvailableUpdate()).resolves.toBeNull();
});

it("silently tolerates unavailable or misconfigured update metadata", async () => {
  vi.mocked(check).mockRejectedValue(new Error("placeholder public key"));

  await expect(findAvailableUpdate()).resolves.toBeNull();
});

it("downloads and relaunches only through the official plugin flow", async () => {
  const update = {
    downloadAndInstall: vi.fn().mockResolvedValue(undefined),
  } as unknown as Update;

  await installAndRelaunch(update);

  expect(update.downloadAndInstall).toHaveBeenCalledOnce();
  expect(relaunch).toHaveBeenCalledOnce();
});
