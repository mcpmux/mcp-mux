ALTER TABLE installed_servers
  ADD COLUMN update_policy    TEXT NOT NULL DEFAULT 'notify';
ALTER TABLE installed_servers
  ADD COLUMN pinned_version   TEXT;
