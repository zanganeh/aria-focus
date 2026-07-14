-- Older builds could leave catalogue children behind after a pack directory was
-- superseded. They cannot be played without their parent pack and the stricter
-- schema below correctly rejects them, so remove only those unreachable rows
-- before rebuilding the catalogue tables.
DELETE FROM item_activity_feedback
WHERE NOT EXISTS (
    SELECT 1 FROM installed_items i WHERE i.item_id = item_activity_feedback.item_id
);
DELETE FROM item_activity_enjoyment
WHERE NOT EXISTS (
    SELECT 1 FROM installed_items i WHERE i.item_id = item_activity_enjoyment.item_id
);
DELETE FROM installed_taxonomy
WHERE NOT EXISTS (
    SELECT 1 FROM installed_packs p WHERE p.pack_id = installed_taxonomy.pack_id
);
DELETE FROM installed_items
WHERE NOT EXISTS (
    SELECT 1 FROM installed_packs p WHERE p.pack_id = installed_items.pack_id
);

ALTER TABLE item_activity_feedback RENAME TO item_activity_feedback_old;
ALTER TABLE item_activity_enjoyment RENAME TO item_activity_enjoyment_old;
ALTER TABLE installed_items RENAME TO installed_items_old;
ALTER TABLE installed_taxonomy RENAME TO installed_taxonomy_old;
ALTER TABLE installed_packs RENAME TO installed_packs_old;

CREATE TABLE installed_packs (
    pack_id TEXT PRIMARY KEY, title TEXT NOT NULL, version TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL, archive_sha256 TEXT NOT NULL,
    install_path TEXT NOT NULL UNIQUE, item_count INTEGER NOT NULL CHECK (item_count >= 0),
    status TEXT NOT NULL CHECK (status IN ('validated_metadata', 'owner_waived_bundled_private_beta', 'generated_local')),
    canonical_manifest TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE installed_items (item_id TEXT PRIMARY KEY, pack_id TEXT NOT NULL REFERENCES installed_packs(pack_id) ON DELETE CASCADE, title TEXT NOT NULL);
CREATE TABLE generated_local_evidence (
    pack_id TEXT PRIMARY KEY REFERENCES installed_packs(pack_id) ON DELETE CASCADE,
    generation_job_id TEXT NOT NULL UNIQUE,
    evidence_json TEXT NOT NULL,
    created_at_unix_seconds INTEGER NOT NULL
);
CREATE TABLE installed_taxonomy (pack_id TEXT NOT NULL REFERENCES installed_packs(pack_id) ON DELETE CASCADE, kind TEXT NOT NULL CHECK (kind IN ('genre', 'mood')), term_id TEXT NOT NULL, label TEXT NOT NULL, PRIMARY KEY (pack_id, kind, term_id));
CREATE TABLE item_activity_feedback (item_id TEXT NOT NULL REFERENCES installed_items(item_id) ON DELETE CASCADE, activity TEXT NOT NULL CHECK (activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')), feedback TEXT NOT NULL CHECK (feedback IN ('helps_focus', 'neutral', 'distracting')), updated_at_unix_seconds INTEGER NOT NULL, PRIMARY KEY (item_id, activity));
CREATE TABLE item_activity_enjoyment (item_id TEXT NOT NULL REFERENCES installed_items(item_id) ON DELETE CASCADE, activity TEXT NOT NULL CHECK (activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')), enjoyment TEXT NOT NULL CHECK (enjoyment IN ('liked', 'not_for_me')), updated_at_unix_seconds INTEGER NOT NULL, PRIMARY KEY (item_id, activity));
INSERT INTO installed_packs(pack_id,title,version,manifest_sha256,archive_sha256,install_path,item_count,status,canonical_manifest) SELECT * FROM installed_packs_old;
INSERT INTO installed_items SELECT * FROM installed_items_old;
INSERT INTO installed_taxonomy SELECT * FROM installed_taxonomy_old;
INSERT INTO item_activity_feedback SELECT * FROM item_activity_feedback_old;
INSERT INTO item_activity_enjoyment SELECT * FROM item_activity_enjoyment_old;
DROP TABLE item_activity_feedback_old; DROP TABLE item_activity_enjoyment_old; DROP TABLE installed_items_old; DROP TABLE installed_taxonomy_old; DROP TABLE installed_packs_old;
