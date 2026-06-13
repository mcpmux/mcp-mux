import { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, KeyRound, Loader2, ShieldCheck, Trash2 } from 'lucide-react';
import { Button, Card, CardContent, CardHeader, CardTitle, Switch } from '@mcpmux/ui';
import {
  getMetaToolsRequireApproval,
  listMetaToolGrants,
  revokeMetaToolGrant,
  setMetaToolsRequireApproval,
  type MetaToolGrantEntry,
} from '@/lib/api/metaTools';

/**
 * Approvals for the `mcpmux_*` self-management writes:
 *   1. The master "Require approval" switch — persisted; OFF auto-approves
 *      every write on this (trusted, local) machine.
 *   2. The session-scoped "always allow (client, tool)" grants, which live in
 *      the gateway's in-memory broker and wipe on restart — shown for
 *      awareness with a panic-revoke button.
 *
 * Refetches on mount and polls every 10s because the broker state can change
 * from either side (dialog clicks or calls to `revokeMetaToolGrant`).
 */
export function MetaToolGrantsPanel() {
  const [grants, setGrants] = useState<MetaToolGrantEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [requireApproval, setRequireApproval] = useState<boolean | null>(null);

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
    getMetaToolsRequireApproval()
      .then(setRequireApproval)
      .catch(() => setRequireApproval(true));
  }, []);

  const handleToggleRequireApproval = async (required: boolean) => {
    const prev = requireApproval;
    setRequireApproval(required);
    try {
      await setMetaToolsRequireApproval(required);
    } catch (e) {
      setRequireApproval(prev);
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
          <ShieldCheck className="h-4 w-4" />
          Tool-management approvals
        </CardTitle>
        <p className="mt-1 text-xs text-[rgb(var(--muted))]">
          Control approval for the <code className="font-mono">mcpmux_*</code> writes a connected
          AI can make (create/update/delete feature sets, bind a workspace).
        </p>
      </CardHeader>
      <CardContent>
        {error && <div className="mb-2 text-sm text-red-600 dark:text-red-400">{error}</div>}

        {/* Master switch — persisted across restarts. OFF auto-approves every
            write on this machine. */}
        <div
          className={`mb-4 flex items-start justify-between gap-3 rounded-lg border p-3 ${
            requireApproval === false
              ? 'border-amber-300/60 bg-amber-50 dark:border-amber-700/50 dark:bg-amber-900/20'
              : 'border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]'
          }`}
          data-testid="meta-tool-require-approval"
        >
          <div className="flex min-w-0 gap-2">
            {requireApproval === false ? (
              <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
            ) : (
              <ShieldCheck className="mt-0.5 h-4 w-4 flex-shrink-0 text-emerald-600 dark:text-emerald-400" />
            )}
            <div className="min-w-0">
              <div className="text-sm font-medium">Require approval for tool changes</div>
              <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">
                {requireApproval === false ? (
                  <span className="text-amber-800 dark:text-amber-300">
                    Off — every <code className="font-mono">mcpmux_*</code> write is applied without
                    asking. Only leave this off on a machine where you trust every connected client.
                  </span>
                ) : (
                  <>
                    On — each <code className="font-mono">mcpmux_*</code> write prompts you to Allow
                    or Deny. Turn off to auto-approve on a trusted machine.
                  </>
                )}
              </p>
            </div>
          </div>
          <Switch
            checked={requireApproval ?? true}
            disabled={requireApproval === null}
            onCheckedChange={(v) => void handleToggleRequireApproval(v)}
            data-testid="meta-tool-require-approval-toggle"
          />
        </div>

        <div className="mb-2 flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-[rgb(var(--muted))]">
          <KeyRound className="h-3 w-3" />
          Session &quot;always allow&quot; grants
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
          <ul className="max-h-64 divide-y divide-[rgb(var(--border-subtle))] overflow-y-auto">
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
