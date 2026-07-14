CREATE TABLE installed_packs (
    pack_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    version TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL,
    archive_sha256 TEXT NOT NULL,
    install_path TEXT NOT NULL UNIQUE,
    item_count INTEGER NOT NULL CHECK (item_count >= 0),
    status TEXT NOT NULL CHECK (status = 'validated_metadata'),
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
