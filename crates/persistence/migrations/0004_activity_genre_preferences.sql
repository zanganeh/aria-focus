CREATE TABLE activity_genre_preferences (
    activity TEXT PRIMARY KEY CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    genre_id TEXT NOT NULL
);
