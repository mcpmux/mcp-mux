import { describe, it, expect } from 'vitest';
import {
  longestPrefixBinding,
  resolveRootBinding,
} from '@/features/workspaces/prefixMatch';
import type { WorkspaceBinding } from '@/lib/api/workspaceBindings';

function binding(root: string): WorkspaceBinding {
  return {
    id: `id:${root}`,
    workspace_root: root,
    space_id: 'space',
    feature_set_ids: ['fs'],
    created_at: '',
    updated_at: '',
  };
}

describe('longestPrefixBinding', () => {
  const bindings = [binding('d:\\mcpmux'), binding('d:\\ionash'), binding('d:\\ionash\\iv')];

  it('matches an exact root', () => {
    expect(longestPrefixBinding('d:\\mcpmux', bindings)?.workspace_root).toBe('d:\\mcpmux');
  });

  it('inherits from an ancestor (the resolver behavior the card must mirror)', () => {
    // d:\mcpmux\mcp-mux has no binding of its own → inherits d:\mcpmux.
    expect(longestPrefixBinding('d:\\mcpmux\\mcp-mux', bindings)?.workspace_root).toBe(
      'd:\\mcpmux'
    );
  });

  it('picks the LONGEST matching prefix', () => {
    expect(longestPrefixBinding('d:\\ionash\\iv\\src', bindings)?.workspace_root).toBe(
      'd:\\ionash\\iv'
    );
  });

  it('respects path-component boundaries (no sibling-prefix false match)', () => {
    // "d:\mcpmux-other" starts with "d:\mcpmux" but the next char is '-', not a
    // separator → must NOT match.
    expect(longestPrefixBinding('d:\\mcpmux-other', bindings)).toBeNull();
  });

  it('returns null when nothing matches', () => {
    expect(longestPrefixBinding('d:\\elsewhere', bindings)).toBeNull();
  });

  it('matches POSIX roots with forward-slash boundaries', () => {
    const posix = [binding('/work/proj')];
    expect(longestPrefixBinding('/work/proj/src', posix)?.workspace_root).toBe('/work/proj');
    expect(longestPrefixBinding('/work/project', posix)).toBeNull();
  });
});

describe('resolveRootBinding', () => {
  const bindings = [binding('d:\\mcpmux')];

  it('reports an exact own binding', () => {
    const r = resolveRootBinding('d:\\mcpmux', bindings);
    expect(r.exact?.workspace_root).toBe('d:\\mcpmux');
    expect(r.effective?.workspace_root).toBe('d:\\mcpmux');
  });

  it('reports inheritance (no exact, effective from ancestor)', () => {
    const r = resolveRootBinding('d:\\mcpmux\\mcp-mux', bindings);
    expect(r.exact).toBeNull();
    expect(r.effective?.workspace_root).toBe('d:\\mcpmux');
  });

  it('reports nothing for an unmapped root', () => {
    const r = resolveRootBinding('d:\\nope', bindings);
    expect(r.exact).toBeNull();
    expect(r.effective).toBeNull();
  });
});
