import type { Intensity } from "./types";

export const INTENSITY_ORDER: Intensity[] = ["off", "low", "medium", "high"];

export const INTENSITY_LABELS: Record<Intensity, string> = {
  off: "Off",
  low: "Low",
  medium: "Medium",
  high: "High / ADHD",
};

export const INTENSITY_ARIA: Record<Intensity, string> = {
  off: "No stimulation processing",
  low: "Subtle stimulation, level 1 of 3",
  medium: "Default functional profile, level 2 of 3",
  high: "Strongest profile, level 3 of 3, opt-in",
};

/** Level number shown alongside the label so the indicator never relies on
 *  colour alone. */
export function intensityLevel(i: Intensity): number {
  return INTENSITY_ORDER.indexOf(i);
}

/** Format a duration in seconds as `H:MM:SS` or `M:SS`. */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  const seconds = s % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${minutes}:${pad(seconds)}`;
}
