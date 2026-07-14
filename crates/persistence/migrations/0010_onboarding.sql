CREATE TABLE onboarding_preferences (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    intensity TEXT NOT NULL CHECK (intensity IN ('low', 'medium', 'high'))
);

CREATE TABLE onboarding_genres (
    genre_id TEXT PRIMARY KEY NOT NULL,
    position INTEGER NOT NULL UNIQUE CHECK (position BETWEEN 0 AND 2)
);

-- Existing profiles have already made a meaningful local choice, so do not
-- interrupt their normal startup with a new first-run flow.
INSERT INTO onboarding_preferences(singleton, completed, intensity)
SELECT 1,
       CASE WHEN EXISTS(SELECT 1 FROM application_preferences)
                  OR EXISTS(SELECT 1 FROM activity_preferences)
                  OR EXISTS(SELECT 1 FROM activity_timer_preferences)
                  OR EXISTS(SELECT 1 FROM activity_genre_preferences)
                  OR EXISTS(SELECT 1 FROM activity_mood_preferences)
                  OR EXISTS(SELECT 1 FROM item_activity_feedback)
                  OR EXISTS(SELECT 1 FROM item_activity_enjoyment)
                  OR EXISTS(SELECT 1 FROM application_preferences WHERE master_volume <> 70)
            THEN 1 ELSE 0 END,
       'medium';
