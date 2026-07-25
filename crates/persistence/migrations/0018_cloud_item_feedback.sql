-- Cloud-generated tracks are activated from a verified file-backed library,
-- not registered as installed content-pack items. Keep their feedback in a
-- separate durable ledger so the installed-pack trust boundary stays closed.
CREATE TABLE cloud_item_activity_feedback (
    item_id TEXT NOT NULL CHECK(length(item_id) BETWEEN 16 AND 128),
    activity TEXT NOT NULL CHECK(activity IN ('deep_work','motivation','creativity','learning','light_work')),
    feedback TEXT NOT NULL CHECK(feedback IN ('helps_focus','neutral','distracting')),
    updated_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY(item_id, activity)
);

CREATE TABLE cloud_item_activity_enjoyment (
    item_id TEXT NOT NULL CHECK(length(item_id) BETWEEN 16 AND 128),
    activity TEXT NOT NULL CHECK(activity IN ('deep_work','motivation','creativity','learning','light_work')),
    enjoyment TEXT NOT NULL CHECK(enjoyment IN ('liked','not_for_me')),
    updated_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY(item_id, activity)
);
