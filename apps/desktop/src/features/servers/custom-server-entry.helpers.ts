import USER_SPACE_SCHEMA from '../../../../../schemas/user-space.schema.json';

/** Default dictionary key stem for new custom servers. */
export const CUSTOM_SERVER_BASE_KEY = 'custom-server';

/** Parsed space JSON config shape (minimal subset used by custom-server flows). */
export type SpaceConfigJson = {
  mcpServers?: Record<string, unknown>;
  [key: string]: unknown;
};

/** Guided form state — full fields land in Phase 3. */
export interface CustomServerFormState {
  serverId: string;
}

/**
 * Pick the next unused `mcpServers` key by appending `-2`, `-3`, … to `base`.
 */
export function nextCustomServerKey(
  servers: Record<string, unknown>,
  base: string = CUSTOM_SERVER_BASE_KEY,
): string {
  let suffix = 1;

  while (true) {
    const key = suffix === 1 ? base : `${base}-${suffix}`;
    if (!(key in servers)) {
      return key;
    }
    suffix += 1;
  }
}

/**
 * Human-readable name suffix when the key was collision-resolved (e.g. `custom-server-2` → ` 2`).
 */
export function customServerNameSuffix(
  key: string,
  base: string = CUSTOM_SERVER_BASE_KEY,
): string {
  if (key === base) {
    return '';
  }
  return ` ${key.replace(`${base}-`, '')}`;
}

/**
 * Default stdio stub entry for a new custom server at `serverKey`.
 */
export function createDefaultStdioEntry(
  serverKey: string,
  base: string = CUSTOM_SERVER_BASE_KEY,
): Record<string, unknown> {
  const suffix = customServerNameSuffix(serverKey, base);
  return {
    name: `New Custom Server${suffix}`,
    command: '',
    args: [],
    env: {},
  };
}

/**
 * Merge a built server entry into space config at `key` (overwrites if present).
 */
export function upsertServerEntry(
  config: SpaceConfigJson,
  key: string,
  entry: Record<string, unknown>,
): SpaceConfigJson {
  const mcpServers = { ...(config.mcpServers ?? {}) };
  mcpServers[key] = entry;
  return { ...config, mcpServers };
}

/**
 * Assemble a server config entry from guided form state (Phase 3).
 */
export function buildServerEntryFromForm(_form: CustomServerFormState): Record<string, unknown> {
  throw new Error('buildServerEntryFromForm is not implemented until Phase 3');
}

/**
 * Monaco JSON schema root for validating a single `mcpServers` entry (not the whole file).
 */
export const SINGLE_SERVER_ENTRY_SCHEMA = {
  ...USER_SPACE_SCHEMA.$defs.serverConfig,
  $defs: {
    stdioServer: USER_SPACE_SCHEMA.$defs.stdioServer,
    httpServer: USER_SPACE_SCHEMA.$defs.httpServer,
    metadata: USER_SPACE_SCHEMA.$defs.metadata,
    inputDef: USER_SPACE_SCHEMA.$defs.inputDef,
  },
};
