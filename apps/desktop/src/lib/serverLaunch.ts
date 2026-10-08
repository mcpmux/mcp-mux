import type { TransportConfig } from '@/types/registry';

/** Quote one argument for display the way a POSIX shell would need it. */
function shellQuote(arg: string): string {
  return /^[A-Za-z0-9_@%+=:,./-]+$/.test(arg) ? arg : `'${arg.replace(/'/g, `'\\''`)}'`;
}

/**
 * What a server definition will actually run or connect to: the full
 * command line for a local (stdio) server, the URL for a remote one.
 * `${input:…}` placeholders are shown as they are; their values are
 * filled in from what the user enters.
 */
export function describeLaunch(transport: TransportConfig): string {
  if (transport.type === 'stdio') {
    return [transport.command, ...(transport.args ?? [])].map(shellQuote).join(' ');
  }
  return transport.url;
}
