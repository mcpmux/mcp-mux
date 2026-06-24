-- Per-member flag: when set on an included tool, promote into client tools/list.
ALTER TABLE feature_set_members ADD COLUMN surfaced INTEGER NOT NULL DEFAULT 0;
