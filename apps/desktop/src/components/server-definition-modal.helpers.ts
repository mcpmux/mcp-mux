import type { ServerViewModel, ServerDefinition } from '../types/registry';

const RUNTIME_SERVER_FIELDS = [
  'is_installed',
  'enabled',
  'oauth_connected',
  'input_values',
  'connection_status',
  'missing_required_inputs',
  'last_error',
  'created_at',
  'installation_source',
  'env_overrides',
  'args_append',
  'extra_headers',
  'default_params',
] as const;

/** Extract only ServerDefinition fields, stripping runtime state */
export function extractDefinition(server: ServerViewModel): ServerDefinition {
  const copy = { ...server };
  for (const key of RUNTIME_SERVER_FIELDS) {
    delete (copy as Record<string, unknown>)[key];
  }
  return copy as ServerDefinition;
}

/**
 * Build the standard MCP config format (the shape that lives under a
 * `mcpServers` key in a space JSON file) from a server's current view model.
 * This is the editable subset — no id/source/badges or other derived fields.
 */
export function buildEditableEntry(server: ServerViewModel): Record<string, unknown> {
  const entry: Record<string, unknown> = {};

  if (server.transport.type === 'stdio') {
    entry.command = server.transport.command;
    entry.args = server.transport.args;
    entry.env = server.transport.env;
  } else {
    entry.url = server.transport.url;
    entry.headers = server.transport.headers;
  }

  entry.name = server.name;
  if (server.description) entry.description = server.description;
  if (server.icon) entry.icon = server.icon;
  if (server.alias) entry.alias = server.alias;
  if (server.auth && server.auth.type !== 'none') entry.auth = server.auth;
  if (server.transport.metadata.inputs.length > 0) {
    entry.metadata = { inputs: server.transport.metadata.inputs };
  }

  return entry;
}

/** Whether the Definition editor allows in-place edits for this server. */
export function canEditServerDefinition(server: ServerViewModel): boolean {
  return (
    server.source.type === 'UserSpace' || server.installation_source?.type === 'manual_entry'
  );
}
