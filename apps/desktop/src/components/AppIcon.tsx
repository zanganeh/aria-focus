import type { ReactNode } from "react";

type Name =
  | "home"
  | "library"
  | "create"
  | "history"
  | "settings"
  | "chevron-left"
  | "sliders"
  | "previous"
  | "next"
  | "pause"
  | "play"
  | "stop"
  | "speaker"
  | "speaker-muted";

const icons: Record<Name, ReactNode> = {
  home: (
    <path d="M3 10.8 12 3l9 7.8v9.7a.5.5 0 0 1-.5.5h-5.1v-5.4H8.6V20H3.5a.5.5 0 0 1-.5-.5v-8.7Z" />
  ),
  library: (
    <path d="M5 4.5h12.3a1.7 1.7 0 0 1 1.7 1.7v13.3H6.7A1.7 1.7 0 0 1 5 17.8V4.5Zm0 0v13.3A1.7 1.7 0 0 0 6.7 19.5H19M8.5 8h7M8.5 11.5h7" />
  ),
  create: (
    <>
      <path d="m12 3 1.2 3.3L16.5 7.5l-3.3 1.2L12 12l-1.2-3.3-3.3-1.2 3.3-1.2L12 3Z" />
      <path d="m18.2 12.5.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2ZM6.5 13l.9 2.6 2.6.9-2.6.9L6.5 20l-.9-2.6-2.6-.9 2.6-.9.9-2.6Z" />
    </>
  ),
  history: <path d="M4 12a8 8 0 1 0 2.3-5.7L4 8.6M4 4.8v3.8h3.8M12 7.7V12l3.1 2" />,
  settings: (
    <path d="M12 8.3a3.7 3.7 0 1 0 0 7.4 3.7 3.7 0 0 0 0-7.4Zm0-5.3 1 2.1 2.3.5 1.8-1.4 2.1 2.1-1.4 1.8.5 2.3 2.1 1v3l-2.1 1-.5 2.3 1.4 1.8-2.1 2.1-1.8-1.4-2.3.5-1 2.1H9l-1-2.1-2.3-.5-1.8 1.4-2.1-2.1 1.4-1.8-.5-2.3-2.1-1v-3l2.1-1 .5-2.3-1.4-1.8 2.1-2.1 1.8 1.4 2.3-.5 1-2.1h3Z" />
  ),
  "chevron-left": <path d="m14.5 5-7 7 7 7" />,
  sliders: <path d="M4 7h16M7 4v6m-3 17h16m-5-3v6M4 12h16m-11-3v6" />,
  previous: (
    <>
      <rect x="5" y="5" width="2.4" height="14" rx="0.8" />
      <path d="M19 5 9 12l10 7Z" />
    </>
  ),
  next: (
    <>
      <path d="M5 5l10 7-10 7Z" />
      <rect x="16.6" y="5" width="2.4" height="14" rx="0.8" />
    </>
  ),
  pause: (
    <>
      <rect x="6.4" y="5" width="3.4" height="14" rx="1" />
      <rect x="14.2" y="5" width="3.4" height="14" rx="1" />
    </>
  ),
  play: <path d="M8 5l11 7-11 7Z" />,
  stop: <rect x="6.5" y="6.5" width="11" height="11" rx="2" />,
  speaker: (
    <>
      <path d="M5 9h3l4-3.6v13.2L8 15H5z" />
      <path d="M15.5 9a4.5 4.5 0 0 1 0 6 M18 6.5a8 8 0 0 1 0 11" />
    </>
  ),
  "speaker-muted": (
    <>
      <path d="M5 9h3l4-3.6v13.2L8 15H5z" />
      <path d="M16.5 9.5l4 4 M20.5 9.5l-4 4" />
    </>
  ),
};

export function AppIcon({ name }: { name: Name }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      {icons[name]}
    </svg>
  );
}
