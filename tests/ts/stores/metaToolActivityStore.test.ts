/**
 * Guards the fix for "Recent meta-tool activity vanished on tab change":
 * rows live in a global store (not component-local state), so this verifies
 * the store's ring-buffer semantics directly.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import {
  useMetaToolActivityStore,
  MAX_META_TOOL_ROWS,
} from '@/stores/metaToolActivityStore';
import type { MetaToolAuditEvent } from '@/lib/api/metaTools';

function ev(tool: string, i: number): MetaToolAuditEvent {
  return {
    client_id: `client-${i}`,
    tool_name: tool,
    decision: 'read',
    summary: '',
    timestamp: new Date(2026, 0, 1, 0, 0, i).toISOString(),
  } as MetaToolAuditEvent;
}

describe('metaToolActivityStore', () => {
  beforeEach(() => useMetaToolActivityStore.getState().clear());

  it('keeps rows most-recent-first', () => {
    const { push } = useMetaToolActivityStore.getState();
    push(ev('first', 1));
    push(ev('second', 2));
    expect(useMetaToolActivityStore.getState().rows.map((r) => r.tool_name)).toEqual([
      'second',
      'first',
    ]);
  });

  it('trims to the ring-buffer size, dropping the oldest', () => {
    const { push } = useMetaToolActivityStore.getState();
    for (let i = 0; i < MAX_META_TOOL_ROWS + 10; i++) push(ev('t', i));
    const rows = useMetaToolActivityStore.getState().rows;
    expect(rows.length).toBe(MAX_META_TOOL_ROWS);
    // Newest kept, oldest evicted.
    expect(rows[0].client_id).toBe(`client-${MAX_META_TOOL_ROWS + 9}`);
    expect(rows.some((r) => r.client_id === 'client-0')).toBe(false);
  });

  it('clear empties the buffer', () => {
    useMetaToolActivityStore.getState().push(ev('a', 1));
    useMetaToolActivityStore.getState().clear();
    expect(useMetaToolActivityStore.getState().rows).toEqual([]);
  });
});
