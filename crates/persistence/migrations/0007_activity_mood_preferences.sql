CREATE TABLE activity_mood_preferences (
    activity TEXT PRIMARY KEY CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    mood_id TEXT NOT NULL
);
