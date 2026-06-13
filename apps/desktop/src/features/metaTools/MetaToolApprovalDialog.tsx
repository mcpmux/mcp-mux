import { useCallback, useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, CheckCircle2, XCircle } from 'lucide-react';
import { Button, Card, CardContent, CardHeader, CardTitle } from '@mcpmux/ui';

/**
 * Incoming approval request emitted by the gateway's ApprovalBroker.
 * Shape mirrors `mcpmux_gateway::services::ApprovalRequest`.
 */
export interface ApprovalRequest {
  request_id: string;
  client_id: string;
  payload: {
    tool_name: string;
    summary: string;
    /**
     * Tool-list diff the dialog renders. Freeform by design — the backend's
     * `ApprovalPayload.diff` is an arbitrary JSON value and each write tool
     * sends a different shape (`mcpmux_create_feature_set` sends
     * `{ added_tools }`; others may send `{ before, after, added, removed }`).
     * Read it defensively (see `toStringArray`); never assume a field exists.
     */
    diff: null | Record<string, unknown>;
    raw_args: unknown;
    affects_other_clients: boolean;
  };
  expires_at_unix_secs: number;
}

/** Coerce a freeform JSON value into a `string[]`, dropping non-strings. */
function toStringArray(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
}

type Decision = 'allow_once' | 'always_for_this_session_and_client' | 'deny';

/**
 * Global listener that renders an approval dialog whenever the gateway
 * asks for permission to run an `mcpmux_*` write tool. Place once, near the
 * root of the app.
 *
 * The dialog queues multiple concurrent requests — if two clients request
 * approval at the same time, the user sees them in order.
 */
export function MetaToolApprovalDialog() {
  const [queue, setQueue] = useState<ApprovalRequest[]>([]);
  const current = queue[0];

  useEffect(() => {
    const unlistenPromise = listen<ApprovalRequest>(
      'meta-tool-approval-request',
      (event) => {
        setQueue((prev) => [...prev, event.payload]);
      }
    );
    return () => {
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const respond = useCallback(
    async (decision: Decision) => {
      if (!current) return;
      try {
        await invoke('respond_to_meta_tool_approval', {
          requestId: current.request_id,
          clientId: current.client_id,
          toolName: current.payload.tool_name,
          decision,
        });
      } catch (e) {
        // Log but don't block UI — broker will time out and surface
        // `approval_timed_out` to the tool caller.
        console.warn('respond_to_meta_tool_approval failed', e);
      } finally {
        setQueue((prev) => prev.slice(1));
      }
    },
    [current]
  );

  // Normalize the freeform diff defensively — a missing field must never
  // throw (this previously crashed on `mcpmux_create_feature_set`, whose diff
  // is `{ added_tools }` and has no `after`).
  const rawDiff = current?.payload.diff ?? null;
  const added = useMemo(
    () => [...toStringArray(rawDiff?.added), ...toStringArray(rawDiff?.added_tools)],
    [rawDiff]
  );
  const removed = useMemo(() => toStringArray(rawDiff?.removed), [rawDiff]);
  const hasBeforeAfter = rawDiff != null && ('before' in rawDiff || 'after' in rawDiff);
  const beforeCount = toStringArray(rawDiff?.before).length;
  const afterCount = hasBeforeAfter ? toStringArray(rawDiff?.after).length : added.length;
  const hasDiff = rawDiff != null && (added.length > 0 || removed.length > 0 || hasBeforeAfter);
  const deltaLabel = `+${added.length} / -${removed.length}`;

  if (!current) return null;

  return (
    <div
      className="fixed inset-0 z-[1000] bg-black/40 backdrop-blur-sm flex items-center justify-center p-4"
      data-testid="meta-tool-approval-dialog"
    >
      <Card className="w-full max-w-xl shadow-2xl">
        <CardHeader className="flex flex-row items-center gap-2">
          <AlertTriangle className="h-5 w-5 text-amber-500" />
          <CardTitle className="text-base">
            An MCP client wants to change your tools
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="text-sm">
            <p className="font-medium">{current.payload.summary}</p>
            <p className="text-xs text-[rgb(var(--muted))] mt-1 font-mono">
              tool:&nbsp;{current.payload.tool_name}
            </p>
          </div>

          {current.payload.affects_other_clients && (
            <div
              className="flex items-start gap-2 p-3 rounded border border-amber-400/40 bg-amber-50/40 dark:bg-amber-900/20 text-xs"
              data-testid="meta-tool-approval-cross-client-warning"
            >
              <AlertTriangle className="h-4 w-4 text-amber-600 mt-0.5 shrink-0" />
              <span>
                This change affects every connection in this Space — not just
                the one requesting it. Other connected clients will see a new
                toolset on their next <code>tools/list</code>.
              </span>
            </div>
          )}

          {hasDiff && (
            <div className="border border-[rgb(var(--border-subtle))] rounded text-xs">
              <div className="grid grid-cols-3 divide-x divide-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
                <Stat label="Before" value={hasBeforeAfter ? beforeCount : '—'} />
                <Stat label="After" value={afterCount} emphasis />
                <Stat label="Delta" value={deltaLabel} />
              </div>
              {(added.length > 0 || removed.length > 0) && (
                <div className="max-h-40 overflow-y-auto p-2 space-y-0.5 font-mono">
                  {added.map((t) => (
                    <div
                      key={`+${t}`}
                      className="text-green-600 dark:text-green-400"
                    >
                      + {t}
                    </div>
                  ))}
                  {removed.map((t) => (
                    <div
                      key={`-${t}`}
                      className="text-red-600 dark:text-red-400"
                    >
                      − {t}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <div className="flex items-center justify-end gap-2 pt-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => respond('deny')}
              data-testid="meta-tool-approval-deny"
            >
              <XCircle className="h-4 w-4 mr-1" /> Deny
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => respond('always_for_this_session_and_client')}
              title="Allow this (client, tool) pair without prompting again until the gateway restarts"
              data-testid="meta-tool-approval-always"
            >
              Always for this session
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={() => respond('allow_once')}
              data-testid="meta-tool-approval-allow-once"
            >
              <CheckCircle2 className="h-4 w-4 mr-1" /> Allow once
            </Button>
          </div>

          {queue.length > 1 && (
            <p className="text-[11px] text-[rgb(var(--muted))] text-right pt-1">
              {queue.length - 1} more pending…
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function Stat({
  label,
  value,
  emphasis,
}: {
  label: string;
  value: number | string;
  emphasis?: boolean;
}) {
  return (
    <div className="p-2 flex flex-col">
      <span className="text-[10px] uppercase tracking-wide text-[rgb(var(--muted))]">
        {label}
      </span>
      <span
        className={
          emphasis
            ? 'text-base font-semibold'
            : 'text-sm font-medium'
        }
      >
        {value}
      </span>
    </div>
  );
}
