CREATE TABLE application_preferences (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_activity TEXT NOT NULL CHECK (
        last_activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    )
);

CREATE TABLE activity_preferences (
    activity TEXT PRIMARY KEY CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    intensity TEXT NOT NULL CHECK (intensity IN ('off', 'low', 'medium', 'high'))
);
