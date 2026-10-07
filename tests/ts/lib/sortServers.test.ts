import { beforeEach, describe, expect, it } from 'vitest';
import {
  isToolsSort,
  readStoredToolsSort,
  sortServers,
  storeToolsSort,
  TOOLS_SORT_STORAGE_KEY,
} from '@/features/servers/sortServers';
import type { ServerViewModel } from '@/types/registry';

function server(overrides: Partial<ServerViewModel> & { name: string }): ServerViewModel {
  return {
    is_installed: true,
    enabled: false,
    oauth_connected: false,
    input_values: {},
    connection_status: 'disconnected',
    missing_required_inputs: false,
    last_error: null,
    ...overrides,
  } as ServerViewModel;
}

const servers: ServerViewModel[] = [
  server({ name: 'postgres', enabled: false, created_at: '2026-01-01T00:00:00Z' }),
  server({ name: 'Filesystem', enabled: true, created_at: '2026-03-01T00:00:00Z' }),
  server({ name: 'brave-search', enabled: true, created_at: '2026-02-01T00:00:00Z' }),
];

const names = (list: ServerViewModel[]) => list.map((s) => s.name);

describe('sortServers', () => {
  it('sorts by name, case-insensitively', () => {
    expect(names(sortServers(servers, 'name'))).toEqual(['brave-search', 'Filesystem', 'postgres']);
  });

  it('sorts by most recently added', () => {
    expect(names(sortServers(servers, 'recent'))).toEqual([
      'Filesystem',
      'brave-search',
      'postgres',
    ]);
  });

  it('places enabled servers first, then falls back to name', () => {
    expect(names(sortServers(servers, 'active-first'))).toEqual([
      'brave-search',
      'Filesystem',
      'postgres',
    ]);
  });

  it('places disabled servers first, then falls back to name', () => {
    expect(names(sortServers(servers, 'inactive-first'))).toEqual([
      'postgres',
      'brave-search',
      'Filesystem',
    ]);
  });

  it('treats servers without a creation date as oldest', () => {
    const undated = server({ name: 'zeta' });
    expect(names(sortServers([...servers, undated], 'recent'))).toEqual([
      'Filesystem',
      'brave-search',
      'postgres',
      'zeta',
    ]);
  });

  it('does not mutate the input', () => {
    const input = [...servers];
    sortServers(input, 'name');
    expect(names(input)).toEqual(['postgres', 'Filesystem', 'brave-search']);
  });
});

describe('tools sort persistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('defaults to recent when nothing is stored', () => {
    expect(readStoredToolsSort()).toBe('recent');
  });

  it('round-trips a stored choice', () => {
    storeToolsSort('name');
    expect(readStoredToolsSort()).toBe('name');
    expect(localStorage.getItem(TOOLS_SORT_STORAGE_KEY)).toBe('name');
  });

  it('falls back to recent for a value this build does not know', () => {
    localStorage.setItem(TOOLS_SORT_STORAGE_KEY, 'by-vibes');
    expect(readStoredToolsSort()).toBe('recent');
  });

  it('recognizes exactly the four supported options', () => {
    expect(isToolsSort('recent')).toBe(true);
    expect(isToolsSort('name')).toBe(true);
    expect(isToolsSort('active-first')).toBe(true);
    expect(isToolsSort('inactive-first')).toBe(true);
    expect(isToolsSort('by-vibes')).toBe(false);
    expect(isToolsSort(null)).toBe(false);
  });
});
