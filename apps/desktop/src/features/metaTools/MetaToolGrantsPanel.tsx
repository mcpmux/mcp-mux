import { useCallback, useEffect, useState } from 'react';
import { FlaskConical, KeyRound, Loader2, Trash2 } from 'lucide-react';
import { Button, Card, CardContent, CardHeader, CardTitle, Switch } from '@mcpmux/ui';
import {
  getMetaToolsAutoApprove,
  listMetaToolGrants,
  revokeMetaToolGrant,
  setMetaToolsAutoApprove,
  type MetaToolGrantEntry,
} from '@/lib/api/metaTools';

/**
 * Session-scoped "always allow (client, tool)" grants. These live in the
 * gateway's in-memory `ApprovalBroker` and are wiped on gateway restart —
 * so showing the list is both for awareness AND for a panic-revoke button
 * when a user regrets ticking "Always for this session".
 *
 * Drop this anywhere. It refetches on mount and polls every 10s because the
 * underlying broker state can change from either side (dialog clicks or
 * calls to `revokeMetaToolGrant`).
 */
export function MetaToolGrantsPanel() {
  const [grants, setGrants] = useState<MetaToolGrantEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [autoApprove, setAutoApprove] = useState<boolean | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await listMetaToolGrants();
      setGrants(data);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    load();
    const i = setInterval(load, 10_000);
    return () => clearInterval(i);
  }, [load]);

  useEffect(() => {
    getMetaToolsAutoApprove()
      .then(setAutoApprove)
      .catch(() => setAutoApprove(false));
  }, []);

  const handleToggleAutoApprove = async (next: boolean) => {
    const prev = autoApprove;
    setAutoApprove(next);
    try {
      await setMetaToolsAutoApprove(next);
    } catch (e) {
      setAutoApprove(prev);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleRevoke = async (g: MetaToolGrantEntry) => {
    const key = `${g.client_id}:${g.tool_name}`;
    setRevoking(key);
    try {
      await revokeMetaToolGrant(g.client_id, g.tool_name);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRevoking(null);
    }
  };

  return (
    <Card data-testid="meta-tool-grants-panel">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <KeyRound className="h-4 w-4" />
          Meta-tool auto-approvals
        </CardTitle>
        <p className="mt-1 text-xs text-[rgb(var(--muted))]">
          &quot;Always for this session&quot; approvals granted to clients for specific{' '}
          <code className="font-mono">mcpmux_*</code> tools. Wipes on gateway restart.
        </p>
      </CardHeader>
      <CardContent>
        {error && <div className="mb-2 text-sm text-red-600 dark:text-red-400">{error}</div>}

        {/* DEBUG: bypass the approval dialog entirely. For developers driving
            mcpmux_* writes (create feature sets / bindings) from a client to
            self-test routing. Session-only; resets on gateway restart. */}
        <div
          className="mb-4 flex items-start justify-between gap-3 rounded-lg border border-amber-300/60 bg-amber-50 p-3 dark:border-amber-700/50 dark:bg-amber-900/20"
          data-testid="meta-tool-auto-approve"
        >
          <div className="flex min-w-0 gap-2">
            <FlaskConical className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
            <div className="min-w-0">
              <div className="text-sm font-medium text-amber-900 dark:text-amber-200">
                Auto-approve writes (debug)
              </div>
              <p className="mt-0.5 text-xs text-amber-800/80 dark:text-amber-300/80">
                Skip the approval dialog for every <code className="font-mono">mcpmux_*</code>{' '}
                write. For self-testing only — resets on gateway restart.
              </p>
            </div>
          </div>
          <Switch
            checked={autoApprove ?? false}
            disabled={autoApprove === null}
            onCheckedChange={(v) => void handleToggleAutoApprove(v)}
            data-testid="meta-tool-auto-approve-toggle"
          />
        </div>

        {grants === null ? (
          <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
            <Loader2 className="h-4 w-4 animate-spin" /> Loading…
          </div>
        ) : grants.length === 0 ? (
          <p className="text-sm italic text-[rgb(var(--muted))]">
            No auto-approvals yet. Each dialog defaults to &quot;Allow once&quot;.
          </p>
        ) : (
          <ul className="divide-y divide-[rgb(var(--border-subtle))]">
            {grants.map((g) => {
              const key = `${g.client_id}:${g.tool_name}`;
              return (
                <li
                  key={key}
                  className="flex items-center justify-between py-2 text-sm"
                  data-testid={`meta-tool-grant-${g.tool_name}`}
                >
                  <div className="mr-3 flex min-w-0 flex-col">
                    <span className="truncate font-mono text-xs">{g.tool_name}</span>
                    <span className="truncate text-[11px] text-[rgb(var(--muted))]">
                      client {g.client_id.slice(0, 8)}…
                    </span>
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => handleRevoke(g)}
                    disabled={revoking === key}
                    data-testid={`meta-tool-grant-revoke-${g.tool_name}`}
                  >
                    {revoking === key ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <>
                        <Trash2 className="mr-1 h-3 w-3" /> Revoke
                      </>
                    )}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
