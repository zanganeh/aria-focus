CREATE TABLE music_studio_job_artifacts (
    job_id TEXT PRIMARY KEY NOT NULL REFERENCES music_studio_jobs(job_id) ON DELETE CASCADE,
    parent_job_id TEXT REFERENCES music_studio_jobs(job_id),
    runtime_version TEXT NOT NULL CHECK(length(runtime_version) BETWEEN 1 AND 80),
    stage TEXT NOT NULL CHECK(stage IN ('queued','generating','analyzing','ready','rejected','failed','cancelled','interrupted')),
    output_relative_path TEXT CHECK(output_relative_path IS NULL OR length(output_relative_path) BETWEEN 1 AND 240),
    output_sha256 TEXT CHECK(output_sha256 IS NULL OR (length(output_sha256) = 64 AND output_sha256 NOT GLOB '*[^0-9a-f]*')),
    analysis_json TEXT CHECK(analysis_json IS NULL OR (json_valid(analysis_json) AND length(analysis_json) <= 16384)),
    safe_error_code TEXT CHECK(safe_error_code IS NULL OR safe_error_code IN ('runtime_invalid','spawn_failed','timeout','gpu_oom','unexpected_exit','output_invalid','analysis_rejected','interrupted')),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms)
);
CREATE INDEX music_studio_job_artifacts_parent ON music_studio_job_artifacts(parent_job_id);
