ALTER TABLE application_preferences ADD COLUMN master_volume INTEGER NOT NULL DEFAULT 70 CHECK(master_volume BETWEEN 0 AND 100);
