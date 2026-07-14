import type { Activity } from "./types";

export interface ActivityCopy {
  label: string;
  need: string;
  direction: string;
}

export const ACTIVITY_ORDER: readonly Activity[] = [
  "deep_work",
  "motivation",
  "creativity",
  "learning",
  "light_work",
];

export const ACTIVITY_COPY: Record<Activity, ActivityCopy> = {
  deep_work: {
    label: "Deep Work",
    need: "Sustained, cognitively demanding work",
    direction: "Low salience, stable density, medium energy",
  },
  motivation: {
    label: "Motivation",
    need: "Starting avoided or low-reward tasks",
    direction: "Brighter and more rhythmic, controlled energy",
  },
  creativity: {
    label: "Creativity",
    need: "Open-ended writing, design, and ideation",
    direction: "Spacious, gently evolving, less rigid pulse",
  },
  learning: {
    label: "Learning",
    need: "Reading, comprehension, and retention",
    direction: "Sparse arrangement, minimal melodic competition",
  },
  light_work: {
    label: "Light Work",
    need: "Email, filing, and repetitive administration",
    direction: "Pleasant, slightly more varied, lower stimulation",
  },
};
