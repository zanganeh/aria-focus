import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CloudGenerationChangedEvent, PlaybackChangedEvent } from "./types";

export const PLAYBACK_CHANGED_EVENT = "aria-focus://playback-changed";
export const CLOUD_GENERATION_CHANGED_EVENT = "aria-focus://cloud-generation-changed";

export function listenPlaybackChanged(
  handler: (payload: PlaybackChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<PlaybackChangedEvent>(PLAYBACK_CHANGED_EVENT, ({ payload }) => handler(payload));
}

export function listenCloudGenerationChanged(
  handler: (payload: CloudGenerationChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<CloudGenerationChangedEvent>(CLOUD_GENERATION_CHANGED_EVENT, ({ payload }) =>
    handler(payload),
  );
}
