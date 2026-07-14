-- Deliberately contains only session-level data.  In particular, no content,
-- review, pack, item, path, or source information may be added to this table.
CREATE TABLE session_history (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32 AND id GLOB '[0-9a-f]*'),
    activity TEXT NOT NULL CHECK(activity IN ('deep_work','motivation','creativity','learning','light_work')),
    intensity TEXT NOT NULL CHECK(intensity IN ('off','low','medium','high')),
    session_type TEXT NOT NULL CHECK(json_valid(session_type)),
    started_at INTEGER NOT NULL CHECK(started_at >= 0),
    ended_at INTEGER CHECK(ended_at IS NULL OR ended_at >= started_at),
    end_reason TEXT CHECK(end_reason IS NULL OR end_reason IN ('stopped','expired','interrupted')),
    focus_seconds INTEGER CHECK(focus_seconds IS NULL OR focus_seconds >= 0),
    focus_outcome TEXT CHECK(focus_outcome IS NULL OR focus_outcome IN ('helped_focus','neutral','distracting')),
    sound_enjoyment TEXT CHECK(sound_enjoyment IS NULL OR sound_enjoyment IN ('liked','not_for_me')),
    CHECK((ended_at IS NULL AND end_reason IS NULL AND focus_seconds IS NULL)
       OR (ended_at IS NOT NULL AND end_reason IS NOT NULL)),
    CHECK(end_reason != 'interrupted' OR focus_seconds IS NULL)
);
CREATE UNIQUE INDEX one_active_session_history ON session_history((1)) WHERE ended_at IS NULL;
CREATE INDEX session_history_recent ON session_history(ended_at DESC, started_at DESC, id DESC);
