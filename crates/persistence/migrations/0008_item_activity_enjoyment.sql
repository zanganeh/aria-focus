CREATE TABLE item_activity_enjoyment (
    item_id TEXT NOT NULL REFERENCES installed_items(item_id) ON DELETE CASCADE,
    activity TEXT NOT NULL CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    enjoyment TEXT NOT NULL CHECK (enjoyment IN ('liked', 'not_for_me')),
    updated_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (item_id, activity)
);
