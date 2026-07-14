CREATE TABLE activity_timer_preferences (
    activity TEXT PRIMARY KEY CHECK (
        activity IN ('deep_work', 'motivation', 'creativity', 'learning', 'light_work')
    ),
    timer_kind TEXT NOT NULL CHECK (timer_kind IN ('infinite', 'countdown', 'interval')),
    countdown_seconds INTEGER,
    work_seconds INTEGER,
    break_seconds INTEGER,
    repeats INTEGER,
    CHECK (
        (timer_kind = 'infinite'
            AND countdown_seconds IS NULL
            AND work_seconds IS NULL
            AND break_seconds IS NULL
            AND repeats IS NULL)
        OR
        (timer_kind = 'countdown'
            AND countdown_seconds BETWEEN 60 AND 28800
            AND work_seconds IS NULL
            AND break_seconds IS NULL
            AND repeats IS NULL)
        OR
        (timer_kind = 'interval'
            AND countdown_seconds IS NULL
            AND work_seconds BETWEEN 60 AND 14400
            AND break_seconds BETWEEN 60 AND 3600
            AND repeats BETWEEN 1 AND 12
            AND (work_seconds * repeats) + (break_seconds * (repeats - 1)) <= 43200)
    )
);
