import { useEffect, useMemo, useState } from "react";
import type { SessionHistoryRecord } from "../lib/types";

function duration(seconds: number | null) {
  if (seconds === null) return "Unavailable";
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export function RecentSessions({ sessions }: { sessions: SessionHistoryRecord[] }) {
  const [page, setPage] = useState(0);
  const pageSize = 4;
  const pageCount = Math.max(1, Math.ceil(sessions.length / pageSize));
  const visibleSessions = useMemo(
    () => sessions.slice(page * pageSize, (page + 1) * pageSize),
    [page, sessions],
  );
  useEffect(() => setPage((current) => Math.min(current, pageCount - 1)), [pageCount]);
  if (!sessions.length) return null;
  return (
    <details className="recent-sessions">
      <summary>Recent sessions</summary>
      <ul aria-label="Recent sessions">
        {visibleSessions.map((session) => (
          <li key={session.id}>
            {session.activity.replace("_", " ")} ·{" "}
            {new Date(session.started_at * 1000).toLocaleString()} ·{" "}
            {duration(session.focus_seconds)} · {session.end_reason}
            {session.focus_outcome ? ` · ${session.focus_outcome}` : ""}
            {session.sound_enjoyment ? ` · ${session.sound_enjoyment}` : ""}
          </li>
        ))}
      </ul>
      {pageCount > 1 && (
        <div className="library-pagination" aria-label="Recent session pages">
          <button
            type="button"
            disabled={page === 0}
            onClick={() => setPage((current) => Math.max(0, current - 1))}
          >
            Previous
          </button>
          <span>
            Page {page + 1} of {pageCount}
          </span>
          <button
            type="button"
            disabled={page + 1 >= pageCount}
            onClick={() => setPage((current) => Math.min(pageCount - 1, current + 1))}
          >
            Next
          </button>
        </div>
      )}
    </details>
  );
}
