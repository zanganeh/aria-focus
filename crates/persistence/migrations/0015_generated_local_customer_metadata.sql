CREATE TABLE generated_local_customer_metadata (
    pack_id TEXT PRIMARY KEY REFERENCES installed_packs(pack_id) ON DELETE CASCADE,
    item_id TEXT NOT NULL UNIQUE REFERENCES installed_items(item_id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 100),
    activity TEXT NOT NULL CHECK(activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')),
    created_at_unix_seconds INTEGER NOT NULL
);
CREATE INDEX generated_local_customer_metadata_created
    ON generated_local_customer_metadata(created_at_unix_seconds DESC, item_id DESC);
