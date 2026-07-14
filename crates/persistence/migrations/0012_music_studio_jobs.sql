-- Local, resumable Music Studio work only. Request and prompt are retained so
-- a restarted application can render the same user-owned job state.
CREATE TABLE music_studio_jobs (
    job_id TEXT PRIMARY KEY NOT NULL
        CHECK(length(job_id) BETWEEN 16 AND 80)
        CHECK(substr(job_id, 1, 4) = 'job_')
        CHECK(job_id NOT GLOB '*[^a-z0-9_]*'),
    attempt_id TEXT NOT NULL UNIQUE
        CHECK(length(attempt_id) BETWEEN 20 AND 80)
        CHECK(substr(attempt_id, 1, 8) = 'attempt_')
        CHECK(attempt_id NOT GLOB '*[^a-z0-9_]*'),
    request_json TEXT NOT NULL CHECK(json_valid(request_json)),
    prompt_json TEXT NOT NULL CHECK(json_valid(prompt_json)),
    state TEXT NOT NULL CHECK(state IN (
        'queued', 'generating', 'analyzing', 'ready', 'rejected', 'failed',
        'cancelled', 'interrupted', 'saving', 'saved'
    )),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms),
    failure_json TEXT CHECK(failure_json IS NULL OR (
        json_valid(failure_json) AND length(failure_json) <= 1024
    ))
);
CREATE INDEX music_studio_jobs_recent
    ON music_studio_jobs(updated_at_ms DESC, created_at_ms DESC, job_id DESC);
