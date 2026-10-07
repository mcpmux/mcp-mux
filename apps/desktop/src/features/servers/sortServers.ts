import type { ServerViewModel } from '@/types/registry';

export type ToolsSort = 'recent' | 'name' | 'active-first' | 'inactive-first';

export const TOOLS_SORT_STORAGE_KEY = 'mcpmux-tools-sort';

const TOOLS_SORTS: readonly ToolsSort[] = ['recent', 'name', 'active-first', 'inactive-first'];

export function isToolsSort(value: string | null): value is ToolsSort {
  return value !== null && TOOLS_SORTS.includes(value as ToolsSort);
}

/** Persisted choice, falling back to "recent" for an unknown or absent value. */
export function readStoredToolsSort(): ToolsSort {
  const saved = localStorage.getItem(TOOLS_SORT_STORAGE_KEY);
  return isToolsSort(saved) ? saved : 'recent';
}

export function storeToolsSort(sort: ToolsSort): void {
  localStorage.setItem(TOOLS_SORT_STORAGE_KEY, sort);
}

function byName(a: ServerViewModel, b: ServerViewModel): number {
  return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
}

function byNewest(a: ServerViewModel, b: ServerViewModel): number {
  const aTime = a.created_at ? new Date(a.created_at).getTime() : 0;
  const bTime = b.created_at ? new Date(b.created_at).getTime() : 0;
  return bTime - aTime || byName(a, b);
}

/**
 * Every ordering falls back to name so equal keys keep a stable, predictable
 * order instead of whatever the merge of registry + installed servers produced.
 */
export function sortServers(servers: ServerViewModel[], sort: ToolsSort): ServerViewModel[] {
  const sorted = [...servers];
  if (sort === 'name') return sorted.sort(byName);
  if (sort === 'active-first') {
    return sorted.sort((a, b) => Number(b.enabled) - Number(a.enabled) || byName(a, b));
  }
  if (sort === 'inactive-first') {
    return sorted.sort((a, b) => Number(a.enabled) - Number(b.enabled) || byName(a, b));
  }
  return sorted.sort(byNewest);
}
