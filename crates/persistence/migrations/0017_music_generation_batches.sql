-- OpenRouter generation is optional online work. Secrets are kept in the OS
-- credential store; this ledger contains only metadata and bounded provider
-- identifiers needed for restart-safe progress and cost accounting.
CREATE TABLE music_generation_batches (
    batch_id TEXT PRIMARY KEY NOT NULL CHECK(length(batch_id) BETWEEN 16 AND 96),
    state TEXT NOT NULL CHECK(state IN (
        'draft','quoted','authorized','running','paused','cancelled','failed',
        'validated','staged','activated','rolled_back'
    )),
    target_count INTEGER NOT NULL CHECK(target_count BETWEEN 1 AND 100),
    budget_microdollars INTEGER NOT NULL CHECK(budget_microdollars >= 0),
    reserved_microdollars INTEGER NOT NULL CHECK(reserved_microdollars >= 0),
    actual_microdollars INTEGER NOT NULL CHECK(actual_microdollars >= 0),
    currency TEXT NOT NULL CHECK(currency = 'USD'),
    catalog_snapshot_json TEXT NOT NULL CHECK(json_valid(catalog_snapshot_json) AND length(catalog_snapshot_json) <= 65536),
    activation_version TEXT CHECK(activation_version IS NULL OR length(activation_version) BETWEEN 1 AND 128),
    previous_activation_version TEXT CHECK(previous_activation_version IS NULL OR length(previous_activation_version) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms),
    error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
    error_message TEXT CHECK(error_message IS NULL OR length(error_message) <= 1024)
);
CREATE INDEX music_generation_batches_recent
    ON music_generation_batches(updated_at_ms DESC, created_at_ms DESC, batch_id DESC);

CREATE TABLE music_generation_batch_items (
    item_id TEXT PRIMARY KEY NOT NULL CHECK(length(item_id) BETWEEN 16 AND 128),
    batch_id TEXT NOT NULL REFERENCES music_generation_batches(batch_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 99),
    activity TEXT NOT NULL CHECK(activity IN ('deep_work','motivation','creativity','learning','light_work')),
    state TEXT NOT NULL CHECK(state IN (
        'queued','refining','generating_audio','generating_cover','validating',
        'validated','failed','cancelled','activated'
    )),
    idempotency_key TEXT NOT NULL UNIQUE CHECK(length(idempotency_key) BETWEEN 16 AND 160),
    prompt_json TEXT NOT NULL CHECK(json_valid(prompt_json) AND length(prompt_json) <= 32768),
    refined_prompt TEXT CHECK(refined_prompt IS NULL OR length(refined_prompt) <= 12000),
    audio_model TEXT NOT NULL CHECK(length(audio_model) BETWEEN 1 AND 160),
    text_model TEXT CHECK(text_model IS NULL OR length(text_model) BETWEEN 1 AND 160),
    image_model TEXT CHECK(image_model IS NULL OR length(image_model) BETWEEN 1 AND 160),
    audio_request_id TEXT CHECK(audio_request_id IS NULL OR length(audio_request_id) <= 160),
    text_request_id TEXT CHECK(text_request_id IS NULL OR length(text_request_id) <= 160),
    image_request_id TEXT CHECK(image_request_id IS NULL OR length(image_request_id) <= 160),
    audio_path TEXT CHECK(audio_path IS NULL OR length(audio_path) <= 240),
    cover_path TEXT CHECK(cover_path IS NULL OR length(cover_path) <= 240),
    audio_sha256 TEXT CHECK(audio_sha256 IS NULL OR (length(audio_sha256) = 64 AND audio_sha256 NOT GLOB '*[^0-9a-f]*')),
    cover_sha256 TEXT CHECK(cover_sha256 IS NULL OR (length(cover_sha256) = 64 AND cover_sha256 NOT GLOB '*[^0-9a-f]*')),
    estimated_microdollars INTEGER NOT NULL CHECK(estimated_microdollars >= 0),
    actual_microdollars INTEGER NOT NULL CHECK(actual_microdollars >= 0),
    validation_json TEXT CHECK(validation_json IS NULL OR (json_valid(validation_json) AND length(validation_json) <= 32768)),
    error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
    error_message TEXT CHECK(error_message IS NULL OR length(error_message) <= 1024),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms),
    UNIQUE(batch_id, ordinal)
);
CREATE INDEX music_generation_batch_items_batch ON music_generation_batch_items(batch_id, ordinal);

CREATE TABLE music_generation_attempts (
    attempt_id TEXT PRIMARY KEY NOT NULL CHECK(length(attempt_id) BETWEEN 16 AND 128),
    item_id TEXT NOT NULL REFERENCES music_generation_batch_items(item_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('text','audio','image')),
    request_id TEXT CHECK(request_id IS NULL OR length(request_id) <= 160),
    estimated_microdollars INTEGER NOT NULL CHECK(estimated_microdollars >= 0),
    actual_microdollars INTEGER NOT NULL CHECK(actual_microdollars >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved','submitted','succeeded','failed','unknown')),
    error_code TEXT CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 64),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= created_at_ms)
);
CREATE INDEX music_generation_attempts_item ON music_generation_attempts(item_id, created_at_ms);
