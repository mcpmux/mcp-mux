ALTER TABLE installed_servers
  ADD COLUMN default_params_strategy TEXT NOT NULL DEFAULT 'fill';
