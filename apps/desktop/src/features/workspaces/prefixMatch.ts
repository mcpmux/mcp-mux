import type { WorkspaceBinding } from '@/lib/api/workspaceBindings';

/**
 * Frontend mirror of `mcpmux_core::longest_prefix_match` — the SAME rule the
 * gateway resolver uses to route a reported root to a binding. Both the
 * `root` and every binding's `workspace_root` are already normalized
 * (drive-letter case, slash direction, no trailing slash) by the time they
 * reach the UI, so we compare them as-is: an exact match, or a binding whose
 * root is a path-component-boundary prefix of `root`. The longest such match
 * wins.
 *
 * This is what makes the Workspaces card agree with the resolver: a folder
 * with no binding of its own still "inherits" an ancestor's binding (e.g.
 * `d:\mcpmux\mcp-mux` inherits `d:\mcpmux`), so it is genuinely mapped and
 * gets tools — not "unmapped".
 */
export function longestPrefixBinding(
  root: string,
  bindings: WorkspaceBinding[]
): WorkspaceBinding | null {
  let best: WorkspaceBinding | null = null;
  for (const b of bindings) {
    const c = b.workspace_root;
    const boundary = root[c.length];
    const matches = root === c || (root.startsWith(c) && (boundary === '/' || boundary === '\\'));
    if (matches && (best === null || c.length > best.workspace_root.length)) {
      best = b;
    }
  }
  return best;
}

/** Resolve a reported root to its own (exact) binding + the binding it
 * effectively resolves through (exact or inherited from an ancestor). */
export function resolveRootBinding(
  root: string,
  bindings: WorkspaceBinding[]
): { exact: WorkspaceBinding | null; effective: WorkspaceBinding | null } {
  const effective = longestPrefixBinding(root, bindings);
  const exact = effective && effective.workspace_root === root ? effective : null;
  return { exact, effective };
}
