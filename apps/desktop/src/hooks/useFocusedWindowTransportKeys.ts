import { useEffect, useRef } from "react";
import type { SessionStatus } from "../lib/types";

type TransportAction = () => Promise<void> | void;

interface Options {
  status: SessionStatus;
  pause: TransportAction;
  resume: TransportAction;
  stop: TransportAction;
  navigationAvailable?: boolean;
  next?: TransportAction;
  previous?: TransportAction;
  reportError: (message: string) => void;
}

function isEditableOrNativeControl(event: KeyboardEvent): boolean {
  const path = event.composedPath();
  return path.some((node) => {
    if (!(node instanceof Element)) return false;
    return (
      node.matches("input, textarea, select, button, a, [contenteditable]") ||
      node.closest("input, textarea, select, button, a, [contenteditable]") !== null
    );
  });
}

/** Handles keyboard events delivered to this app window; it does not register system-wide shortcuts. */
export function useFocusedWindowTransportKeys({
  status,
  pause,
  resume,
  stop,
  navigationAvailable = false,
  next = async () => {},
  previous = async () => {},
  reportError,
}: Options) {
  const current = useRef({
    status,
    pause,
    resume,
    stop,
    navigationAvailable,
    next,
    previous,
    reportError,
  });
  const queue = useRef<Promise<void>>(Promise.resolve());
  current.current = {
    status,
    pause,
    resume,
    stop,
    navigationAvailable,
    next,
    previous,
    reportError,
  };

  useEffect(() => {
    const enqueue = (kind: "toggle" | "stop" | "next" | "previous") => {
      queue.current = queue.current
        .then(async () => {
          const latest = current.current;
          const active = latest.status === "playing" || latest.status === "paused";
          if (!active) return;
          if ((kind === "next" || kind === "previous") && latest.navigationAvailable) {
            await (kind === "next" ? latest.next : latest.previous)();
            if (current.current.navigationAvailable === latest.navigationAvailable) {
              current.current = { ...current.current, navigationAvailable: false };
            }
          } else if (kind === "stop") {
            await latest.stop();
            if (current.current.status === latest.status) {
              current.current = { ...current.current, status: "stopped" };
            }
          } else if (kind === "toggle" && latest.status === "playing") {
            await latest.pause();
            if (current.current.status === latest.status) {
              current.current = { ...current.current, status: "paused" };
            }
          } else if (kind === "toggle") {
            await latest.resume();
            if (current.current.status === latest.status) {
              current.current = { ...current.current, status: "playing" };
            }
          }
        })
        .catch((error: unknown) => {
          current.current.reportError(
            `Unable to ${kind === "stop" ? "stop" : kind === "next" || kind === "previous" ? "change track for" : "change playback for"} the session: ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
        });
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat) return;

      if (event.key === "MediaPlayPause") {
        enqueue("toggle");
        return;
      }
      if (event.key === "MediaStop") {
        enqueue("stop");
        return;
      }
      if (event.key === "MediaTrackNext" && current.current.navigationAvailable) {
        enqueue("next");
        return;
      }
      if (event.key === "MediaTrackPrevious" && current.current.navigationAvailable) {
        enqueue("previous");
        return;
      }

      const isSpace = event.key === " " || event.code === "Space";
      if (
        !isSpace ||
        event.ctrlKey ||
        event.altKey ||
        event.metaKey ||
        isEditableOrNativeControl(event)
      ) {
        return;
      }

      const active = current.current.status === "playing" || current.current.status === "paused";
      if (!active) return;
      event.preventDefault();
      enqueue("toggle");
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
