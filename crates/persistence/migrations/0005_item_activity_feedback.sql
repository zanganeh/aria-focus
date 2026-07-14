CREATE TABLE item_activity_feedback (
    item_id TEXT NOT NULL REFERENCES installed_items(item_id) ON DELETE CASCADE,
    activity TEXT NOT NULL CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    feedback TEXT NOT NULL CHECK (feedback IN ('helps_focus', 'neutral', 'distracting')),
    updated_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (item_id, activity)
);
