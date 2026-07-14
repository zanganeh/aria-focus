ALTER TABLE item_activity_feedback RENAME TO item_activity_feedback_old;
ALTER TABLE installed_items RENAME TO installed_items_old;
ALTER TABLE installed_taxonomy RENAME TO installed_taxonomy_old;
ALTER TABLE installed_packs RENAME TO installed_packs_old;

CREATE TABLE installed_packs (
    pack_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    version TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL,
    archive_sha256 TEXT NOT NULL,
    install_path TEXT NOT NULL UNIQUE,
    item_count INTEGER NOT NULL CHECK (item_count >= 0),
    status TEXT NOT NULL CHECK (status IN ('validated_metadata', 'owner_waived_bundled_private_beta')),
    canonical_manifest TEXT NOT NULL
);

CREATE TABLE installed_items (
    item_id TEXT PRIMARY KEY,
    pack_id TEXT NOT NULL REFERENCES installed_packs(pack_id) ON DELETE CASCADE,
    title TEXT NOT NULL
);

CREATE TABLE installed_taxonomy (
    pack_id TEXT NOT NULL REFERENCES installed_packs(pack_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('genre', 'mood')),
    term_id TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY (pack_id, kind, term_id)
);

CREATE TABLE item_activity_feedback (
    item_id TEXT NOT NULL REFERENCES installed_items(item_id) ON DELETE CASCADE,
    activity TEXT NOT NULL CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    feedback TEXT NOT NULL CHECK (feedback IN ('helps_focus', 'neutral', 'distracting')),
    updated_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (item_id, activity)
);

INSERT INTO installed_packs SELECT * FROM installed_packs_old;
INSERT INTO installed_items SELECT * FROM installed_items_old;
INSERT INTO installed_taxonomy SELECT * FROM installed_taxonomy_old;
INSERT INTO item_activity_feedback SELECT * FROM item_activity_feedback_old;
DROP TABLE item_activity_feedback_old;
DROP TABLE installed_items_old;
DROP TABLE installed_taxonomy_old;
DROP TABLE installed_packs_old;
