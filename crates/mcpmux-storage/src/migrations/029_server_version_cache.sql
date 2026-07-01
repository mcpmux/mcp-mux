ALTER TABLE installed_servers
  ADD COLUMN latest_available_version  TEXT;
ALTER TABLE installed_servers
  ADD COLUMN version_checked_at        TEXT;
