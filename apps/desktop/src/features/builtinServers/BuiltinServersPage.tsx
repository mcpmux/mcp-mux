/**
 * Built-in page (formerly "Built-in Servers").
 *
 * Capabilities McpMux itself provides to connected apps — distinct from the
 * servers the user installs under "Tools". Enabled/disabled **per Space**.
 *
 * Layout = three stages, responsive from narrow windows to ultrawide:
 *   1. The shelf — every built-in capability as a uniform card (live ones
 *      toggleable, future ones "Soon"), so the framework reads as one row,
 *      not one giant card.
 *   2. Detail panel — the selected capability's tools with per-tool switches.
 *   3. Approvals & activity — grants + audit in their own bounded section
 *      (side-by-side on wide screens, stacked on narrow), out of the card.
 */

import { useEffect, useState } from 'react';
import { Switch, useToast, ToastContainer } from '@mcpmux/ui';
import { Sparkles, Brain, Wrench, Eye, Pencil, Boxes, Loader2, ShieldCheck } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import {
  listBuiltinServers,
  setBuiltinServerEnabled,
  setBuiltinToolEnabled,
  type BuiltinServer,
} from '@/lib/api/builtinServers';
import { isTauri } from '@/lib/backend/data/transport';
import { MetaToolAuditLog, MetaToolGrantsPanel } from '@/features/metaTools';
import { useViewSpace, useDefaultSpace } from '@/stores';

const SERVER_ICONS: Record<string, React.ReactNode> = {
  'tool-optimization': <Sparkles className="h-5 w-5" />,
};

interface ComingSoonServer {
  id: string;
  name: string;
  description: string;
  icon: React.ReactNode;
}

const COMING_SOON: ComingSoonServer[] = [
  {
    id: 'memory',
    name: 'Memory',
    description: 'Notes and recall your AI can read and write across every app.',
    icon: <Brain className="h-5 w-5" />,
  },
];

export function BuiltinServersPage() {
  const { toasts, success, error, dismiss } = useToast();

  const viewSpace = useViewSpace();
  const defaultSpace = useDefaultSpace();
  const space = viewSpace ?? defaultSpace;
  const spaceId = space?.id ?? null;

  const [servers, setServers] = useState<BuiltinServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string>('tool-optimization');

  useEffect(() => {
    if (!spaceId) return;
    let cancelled = false;
    setLoading(true);
    listBuiltinServers(spaceId)
      .then((s) => {
        if (!cancelled) setServers(s);
      })
      .catch((e) => {
        if (!cancelled) error('Failed to load built-in servers', String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spaceId]);

  // Refetch when this Space's config changes elsewhere (the gateway forwards
  // `builtin-server-config-changed` after a toggle).
  useEffect(() => {
    if (!spaceId || !isTauri()) return;
    let unlisten: (() => void) | undefined;
    void listen<{ space_id: string }>('builtin-server-config-changed', (e) => {
      if (e.payload.space_id === spaceId) {
        void listBuiltinServers(spaceId)
          .then(setServers)
          .catch(() => {
            /* keep current view; initial load surfaced any error */
          });
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [spaceId]);

  const toggleServer = async (serverId: string, enabled: boolean) => {
    if (!spaceId) return;
    const prev = servers;
    setServers((s) => s.map((x) => (x.id === serverId ? { ...x, enabled } : x)));
    try {
      await setBuiltinServerEnabled(spaceId, serverId, enabled);
      const srv = prev.find((x) => x.id === serverId);
      success(
        `${srv?.name ?? 'Server'} ${enabled ? 'enabled' : 'disabled'}`,
        `For ${space?.name ?? 'this Space'} — connected clients update immediately.`
      );
    } catch (e) {
      setServers(prev);
      error('Failed to save', String(e));
    }
  };

  const toggleTool = async (serverId: string, toolName: string, enabled: boolean) => {
    if (!spaceId) return;
    const prev = servers;
    setServers((s) =>
      s.map((x) =>
        x.id === serverId
          ? { ...x, tools: x.tools.map((t) => (t.name === toolName ? { ...t, enabled } : t)) }
          : x
      )
    );
    try {
      await setBuiltinToolEnabled(spaceId, serverId, toolName, enabled);
    } catch (e) {
      setServers(prev);
      error('Failed to save', String(e));
    }
  };

  const selected = servers.find((s) => s.id === selectedId) ?? servers[0];

  return (
    <div className="flex h-full flex-col" data-testid="builtin-servers-page">
      <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-6 lg:p-8">
        <div className="mx-auto max-w-[1600px]">
          <h1 className="text-2xl font-bold tracking-tight lg:text-3xl">Built-in</h1>
          <p className="mt-1.5 max-w-2xl text-sm leading-relaxed text-[rgb(var(--muted))] lg:text-base">
            Built-in tools and additional features McpMux gives your AI apps — no install needed.
            Self-management ships today; Memory is next. Toggle everything{' '}
            <span className="font-medium text-[rgb(var(--foreground))]">per Space</span>.
          </p>
          {/* Active-Space scope — made prominent so toggles aren't applied to
              the wrong Space. Built-in config is per-Space, but clients route
              to a Space via their workspace-root binding, which may differ
              from the Space selected here. */}
          <div
            className="mt-3 inline-flex items-center gap-2 rounded-lg border border-[rgb(var(--primary))]/30 bg-[rgb(var(--primary))]/10 px-3 py-2"
            data-testid="builtin-active-space"
          >
            <Boxes className="h-4 w-4 flex-shrink-0 text-[rgb(var(--primary))]" />
            <span className="text-sm text-[rgb(var(--muted))]">These settings apply to</span>
            <span className="text-sm font-semibold text-[rgb(var(--foreground))]">
              {space?.name ?? '…'}
            </span>
          </div>
        </div>
      </header>

      <div className="flex-1 overflow-auto p-6 lg:p-8">
        <div className="mx-auto max-w-[1600px] space-y-8">
          {loading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="h-6 w-6 animate-spin text-[rgb(var(--primary))]" />
            </div>
          ) : (
            <>
              {/* 1 — The shelf: every capability, uniform cards. */}
              <section>
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-5">
                  {servers.map((server) => {
                    const isSelected = selected?.id === server.id;
                    const enabledTools = server.tools.filter((t) => t.enabled).length;
                    return (
                      <div
                        key={server.id}
                        role="button"
                        tabIndex={0}
                        onClick={() => setSelectedId(server.id)}
                        onKeyDown={(e) => e.key === 'Enter' && setSelectedId(server.id)}
                        data-testid={`builtin-server-${server.id}`}
                        className={`group relative cursor-pointer overflow-hidden rounded-xl border bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:shadow-md ${
                          isSelected
                            ? 'border-[rgb(var(--primary))]/50'
                            : 'border-[rgb(var(--border-subtle))] hover:border-[rgb(var(--border))]'
                        }`}
                      >
                        <span
                          aria-hidden
                          className={`absolute inset-y-0 left-0 w-1 transition-opacity ${
                            server.enabled ? 'bg-emerald-500' : 'bg-slate-400/60'
                          }`}
                        />
                        <div className="flex items-start justify-between gap-2 pl-2">
                          <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))]">
                            {SERVER_ICONS[server.id] ?? <Boxes className="h-5 w-5" />}
                          </span>
                          {/* Switch sits inside a clickable card — keep its
                              clicks from changing the selection. */}
                          <span onClick={(e) => e.stopPropagation()}>
                            <Switch
                              checked={server.enabled}
                              onCheckedChange={(v) => void toggleServer(server.id, v)}
                              data-testid={`builtin-server-toggle-${server.id}`}
                            />
                          </span>
                        </div>
                        <div className="mt-3 pl-2">
                          <div className="font-semibold">{server.name}</div>
                          <p className="mt-1 line-clamp-2 text-xs text-[rgb(var(--muted))]">
                            {server.description}
                          </p>
                          <div className="mt-2 text-[11px] font-medium text-[rgb(var(--muted-foreground))]">
                            {server.enabled
                              ? `${enabledTools}/${server.tools.length} tools on`
                              : 'Off in this Space'}
                          </div>
                        </div>
                      </div>
                    );
                  })}

                  {COMING_SOON.map((s) => (
                    <div
                      key={s.id}
                      className="rounded-xl border border-dashed border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-4 opacity-75"
                      data-testid={`builtin-server-coming-soon-${s.id}`}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-[rgb(var(--background))] text-[rgb(var(--muted))]">
                          {s.icon}
                        </span>
                        <span className="rounded-md bg-[rgb(var(--background))] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
                          Soon
                        </span>
                      </div>
                      <div className="mt-3">
                        <div className="font-semibold">{s.name}</div>
                        <p className="mt-1 line-clamp-2 text-xs text-[rgb(var(--muted))]">
                          {s.description}
                        </p>
                      </div>
                    </div>
                  ))}
                </div>
              </section>

              {/* 2 — Detail panel for the selected capability. */}
              {selected && (
                <section>
                  <div className="mb-3 flex flex-wrap items-center gap-2">
                    <Wrench className="h-4 w-4 text-[rgb(var(--muted))]" />
                    <h2 className="text-sm font-semibold">
                      {selected.name} — tools ({selected.tools.length})
                    </h2>
                    {!selected.enabled && (
                      <span className="rounded-md bg-slate-500/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-slate-500">
                        server off
                      </span>
                    )}
                  </div>
                  <p
                    className="mb-3 text-xs text-[rgb(var(--muted))]"
                    data-testid="builtin-mux-trigger-tip"
                  >
                    Tip: in your AI client, start a request with{' '}
                    <code className="rounded bg-[rgb(var(--surface))] px-1 py-0.5 font-mono text-[11px]">
                      @mux
                    </code>{' '}
                    so it knows to drive these tools — e.g.{' '}
                    <span className="italic">
                      “@mux build a minimal toolset for this repo”
                    </span>
                    . Reads are silent; writes ask for your approval.
                  </p>
                  <div
                    className={`overflow-hidden rounded-xl border border-[rgb(var(--border))] transition-opacity ${
                      selected.enabled ? '' : 'opacity-50'
                    }`}
                  >
                    <div className="divide-y divide-[rgb(var(--border-subtle))]">
                      {selected.tools.map((t) => (
                        <div key={t.name} className="flex items-start gap-3 px-4 py-2.5">
                          {t.write ? (
                            <Pencil className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-500" />
                          ) : (
                            <Eye className="mt-0.5 h-4 w-4 flex-shrink-0 text-emerald-500" />
                          )}
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="truncate font-mono text-sm">{t.name}</span>
                              <span
                                className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                                  t.write
                                    ? 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300'
                                    : 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300'
                                }`}
                              >
                                {t.write ? 'write · approval' : 'read'}
                              </span>
                            </div>
                            <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">
                              {t.description}
                            </p>
                          </div>
                          <Switch
                            checked={t.enabled}
                            disabled={!selected.enabled}
                            onCheckedChange={(v) => void toggleTool(selected.id, t.name, v)}
                            data-testid={`builtin-tool-toggle-${t.name}`}
                          />
                        </div>
                      ))}
                    </div>
                  </div>
                </section>
              )}

              {/* 3 — Approvals & activity: bounded, out of the cards. */}
              <section>
                <div className="mb-3 flex items-center gap-2">
                  <ShieldCheck className="h-4 w-4 text-[rgb(var(--muted))]" />
                  <h2 className="text-sm font-semibold">Approvals & activity</h2>
                </div>
                <div className="grid grid-cols-1 gap-6 xl:grid-cols-2">
                  <MetaToolGrantsPanel />
                  <MetaToolAuditLog />
                </div>
              </section>
            </>
          )}
        </div>
      </div>

      <ToastContainer toasts={toasts} onClose={dismiss} />
    </div>
  );
}
