import USER_SPACE_SCHEMA from '../../../../../schemas/user-space.schema.json';

/** Default dictionary key stem for new custom servers. */
export const CUSTOM_SERVER_BASE_KEY = 'custom-server';

/** Parsed space JSON config shape (minimal subset used by custom-server flows). */
export type SpaceConfigJson = {
  mcpServers?: Record<string, unknown>;
  [key: string]: unknown;
};

/** Transport kind for guided custom-server creation. */
export type CustomServerTransportType = 'stdio' | 'http';

/** Single key/value row in env or HTTP header editors. */
export type KeyValueFormRow = {
  key: string;
  value: string;
};

/** Input definition row for metadata.inputs builder. */
export type InputDefFormRow = {
  id: string;
  label: string;
  type: 'text' | 'password';
  required: boolean;
  secret: boolean;
};

/** Guided form state for custom server creation. */
export interface CustomServerFormState {
  serverId: string;
  displayName: string;
  transportType: CustomServerTransportType;
  command: string;
  url: string;
  description: string;
  argsText: string;
  envRows: KeyValueFormRow[];
  headerRows: KeyValueFormRow[];
  inputDefs: InputDefFormRow[];
  defaultParamsJson: string;
  defaultParamsStrategy: 'fill' | 'override';
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
 * Seed guided form fields for a new custom server at `serverKey`.
 */
export function createDefaultFormState(
  serverKey: string,
  base: string = CUSTOM_SERVER_BASE_KEY,
): CustomServerFormState {
  const suffix = customServerNameSuffix(serverKey, base);
  return {
    serverId: serverKey,
    displayName: `New Custom Server${suffix}`,
    transportType: 'stdio',
    command: '',
    url: '',
    description: '',
    argsText: '',
    envRows: [],
    headerRows: [],
    inputDefs: [],
    defaultParamsJson: '{}',
    defaultParamsStrategy: 'fill',
  };
}

/**
 * Convert key/value rows into a string map, skipping rows with blank keys.
 */
export function keyValueRowsToRecord(rows: KeyValueFormRow[]): Record<string, string> {
  const result: Record<string, string> = {};
  for (const row of rows) {
    const trimmedKey = row.key.trim();
    if (trimmedKey) {
      result[trimmedKey] = row.value;
    }
  }
  return result;
}

/**
 * Parse newline-delimited args text into a trimmed string array.
 */
export function parseArgsText(argsText: string): string[] {
  return argsText
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/**
 * Build metadata.inputs entries from form rows (drops incomplete rows).
 */
export function buildInputDefsFromFormRows(rows: InputDefFormRow[]): Array<Record<string, unknown>> {
  return rows
    .filter((row) => row.id.trim() && row.label.trim())
    .map((row) => {
      const input: Record<string, unknown> = {
        id: row.id.trim(),
        label: row.label.trim(),
        type: row.type,
      };
      if (row.required) {
        input.required = true;
      }
      if (row.secret) {
        input.secret = true;
      }
      return input;
    });
}

/**
 * Parse default params JSON; returns undefined when empty or invalid.
 */
export function parseDefaultParamsJson(
  jsonText: string,
): Record<string, unknown> | undefined {
  const trimmed = jsonText.trim();
  if (!trimmed || trimmed === '{}') {
    return undefined;
  }
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    return undefined;
  }
  return undefined;
}

/**
 * Assemble a server config entry from guided form state (stdio or http shape).
 */
export function buildServerEntryFromForm(form: CustomServerFormState): Record<string, unknown> {
  const entry: Record<string, unknown> = {
    name: form.displayName.trim() || 'New Custom Server',
  };

  const description = form.description.trim();
  if (description) {
    entry.description = description;
  }

  if (form.transportType === 'stdio') {
    entry.command = form.command.trim();
    const args = parseArgsText(form.argsText);
    if (args.length > 0) {
      entry.args = args;
    }
    const env = keyValueRowsToRecord(form.envRows);
    if (Object.keys(env).length > 0) {
      entry.env = env;
    }
  } else {
    entry.url = form.url.trim();
    const headers = keyValueRowsToRecord(form.headerRows);
    if (Object.keys(headers).length > 0) {
      entry.headers = headers;
    }
  }

  const inputs = buildInputDefsFromFormRows(form.inputDefs);
  if (inputs.length > 0) {
    entry.metadata = { inputs };
  }

  const defaultParams = parseDefaultParamsJson(form.defaultParamsJson);
  if (defaultParams && Object.keys(defaultParams).length > 0) {
    entry.default_params = defaultParams;
    if (form.defaultParamsStrategy !== 'fill') {
      entry.default_params_strategy = form.defaultParamsStrategy;
    }
  }

  return entry;
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
