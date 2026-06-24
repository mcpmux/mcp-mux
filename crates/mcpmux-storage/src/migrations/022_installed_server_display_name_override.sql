-- User-supplied display name that survives user-config sync (UI-preferred label).
ALTER TABLE installed_servers ADD COLUMN display_name_override TEXT;
