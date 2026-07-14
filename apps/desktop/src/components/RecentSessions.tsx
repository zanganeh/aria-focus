import type { SessionHistoryRecord } from "../lib/types";

function duration(seconds: number | null) {
  if (seconds === null) return "Unavailable";
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export function RecentSessions({ sessions }: { sessions: SessionHistoryRecord[] }) {
  if (!sessions.length) return null;
  return (
    <details className="recent-sessions">
      <summary>Recent sessions</summary>
      <ul aria-label="Recent sessions">
        {sessions.map((session) => (
          <li key={session.id}>
            {session.activity.replace("_", " ")} ·{" "}
            {new Date(session.started_at * 1000).toLocaleString()} ·{" "}
            {duration(session.focus_seconds)} · {session.end_reason}
            {session.focus_outcome ? ` · ${session.focus_outcome}` : ""}
            {session.sound_enjoyment ? ` · ${session.sound_enjoyment}` : ""}
          </li>
        ))}
      </ul>
    </details>
  );
}
