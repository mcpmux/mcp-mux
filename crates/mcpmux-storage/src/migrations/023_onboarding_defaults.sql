-- Migration 023: "install, connect, use" onboarding defaults.
--
-- 1. FeatureSets gain an `auto_include` flag. An auto set has no explicit
--    members: it grants every feature (tools, prompts, resources) from every
--    server in its Space, so a server installed later shows up in it with no
--    extra step. The first explicit membership edit turns it into a manual
--    set (see SqliteFeatureSetRepository). New Spaces seed their Starter in
--    auto mode (FeatureSet::new_starter).
--
--    On upgrade only an untouched install switches to auto: a Starter with no
--    members on a DB with no installed servers. An existing install keeps its
--    Starter as is, because an empty Starter there may be a deliberate "grant
--    nothing by default". The FeatureSets page offers the switch instead.
--
-- 2. Inbound auth. A missing `gateway.auth_disabled` setting now means "off
--    while the gateway is loopback-only" (see load_gateway_auth_disabled), so
--    a new user can connect an app with no access key. Existing installs that
--    already have clients or servers are pinned to auth-required so the
--    upgrade doesn't silently drop their auth. INSERT OR IGNORE keeps an
--    explicit choice (either value) the user already made.

ALTER TABLE feature_sets ADD COLUMN auto_include INTEGER NOT NULL DEFAULT 0;

UPDATE feature_sets
   SET auto_include = 1
 WHERE feature_set_type IN ('starter', 'default')
   AND is_deleted = 0
   AND NOT EXISTS (
       SELECT 1 FROM feature_set_members m WHERE m.feature_set_id = feature_sets.id
   )
   AND NOT EXISTS (SELECT 1 FROM installed_servers);

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
SELECT 'gateway.auth_disabled', 'false', datetime('now')
 WHERE EXISTS (SELECT 1 FROM inbound_clients)
    OR EXISTS (SELECT 1 FROM installed_servers);
