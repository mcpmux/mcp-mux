import type { TransportConfig } from '@/types/registry';

/** Control, format (bidi, zero-width) and line/paragraph-separator characters. */
const HIDDEN_CHARS = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/gu;

/** Write characters that would hide or reorder text as visible `\u{..}` escapes. */
function showHidden(text: string): string {
  return text.replace(HIDDEN_CHARS, (c) => `\\u{${c.codePointAt(0)!.toString(16)}}`);
}

/** Quote one argument for display the way a POSIX shell would need it. */
function shellQuote(arg: string): string {
  const shown = showHidden(arg);
  return /^[A-Za-z0-9_@%+=:,./-]+$/.test(shown) ? shown : `'${shown.replace(/'/g, `'\\''`)}'`;
}

/** Replace `${input:ID}` placeholders whose input has a default with that default. */
function fillDefaults(text: string, defaults: Map<string, string>): string {
  return text.replace(/\$\{input:([A-Za-z_][A-Za-z0-9_]*)\}/g, (match, id: string) =>
    defaults.has(id) ? defaults.get(id)! : match
  );
}

/**
 * What a server definition will actually run or connect to: for a local
 * (stdio) server the environment and the full command line, for a remote one
 * the URL. Mirrors the gateway: input defaults are filled into `${input:…}`
 * placeholders, the registry's env is set, and every input is also exported
 * as an env var named after its id (an input without a default shows as
 * `<your value>`). Characters that would hide part of the text are shown as
 * `\u{..}` escapes.
 */
export function describeLaunch(transport: TransportConfig): string {
  const inputs = transport.metadata?.inputs ?? [];
  const defaults = new Map(
    inputs
      .filter((i) => i.default !== undefined && i.default !== null)
      .map((i) => [i.id, i.default as string])
  );
  if (transport.type === 'stdio') {
    const env = new Map<string, string>();
    for (const [key, value] of Object.entries(transport.env ?? {}).sort(([a], [b]) =>
      a.localeCompare(b)
    )) {
      env.set(key, fillDefaults(value, defaults));
    }
    for (const input of inputs) {
      env.set(input.id, defaults.get(input.id) ?? '<your value>');
    }
    const assignments = [...env].map(([key, value]) => `${showHidden(key)}=${shellQuote(value)}`);
    const command = [transport.command, ...(transport.args ?? [])].map((part) =>
      shellQuote(fillDefaults(part, defaults))
    );
    return [...assignments, ...command].join(' ');
  }
  return showHidden(fillDefaults(transport.url, defaults));
}
