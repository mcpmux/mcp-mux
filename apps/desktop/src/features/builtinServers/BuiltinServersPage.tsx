/**
 * Context page (formerly "Built-in Servers").
 *
 * Home for the capabilities McpMux itself provides to connected apps —
 * distinct from the servers the user installs (those live under "Tools").
 * Built-in servers and their individual tools are enabled/disabled
 * **per Space**: this page configures the Space currently selected in the
 * sidebar's Space switcher.
 *
 * Today there's one concrete built-in server — "Tool Optimization" (the
 * `mcpmux_*` self-management toolset). Memory / Skills / Plugins are scaffolded
 * as "coming soon" — per the superapp plan they all land on this page.
 */

import { useEffect, useState } from 'react';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Switch,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import {
  Sparkles,
  Brain,
  BookOpen,
  Puzzle,
  Wrench,
  Eye,
  Pencil,
  Boxes,
  Loader2,
  Layers,
} from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import {
  listBuiltinServers,
  setBuiltinServerEnabled,
  setBuiltinToolEnabled,
  type BuiltinServer,
} from '@/lib/api/builtinServers';
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
    description: 'Persistent notes and recall the AI can read and write across sessions.',
    icon: <Brain className="h-5 w-5" />,
  },
  {
    id: 'skills',
    name: 'Skills',
    description: 'Search a catalog of skills and pull the ones you want into a Space.',
    icon: <BookOpen className="h-5 w-5" />,
  },
  {
    id: 'plugins',
    name: 'Plugins',
    description: 'Extend McpMux with community plugins exposed as first-class tools.',
    icon: <Puzzle className="h-5 w-5" />,
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
    if (!spaceId) return;
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

  return (
    <div className="flex h-full flex-col" data-testid="builtin-servers-page">
      <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
        <div className="mx-auto max-w-[2000px]">
          <h1 className="text-3xl font-bold">Context</h1>
          <p className="mt-2 max-w-2xl text-base text-[rgb(var(--muted))]">
            Capabilities McpMux itself gives your AI apps — separate from the servers you install
            under <span className="font-medium text-[rgb(var(--foreground))]">Tools</span>.
            Self-management ships today; Memory, Skills, and Plugins are coming. Enable them and
            toggle their tools <span className="font-medium">per Space</span>; the choices below
            apply to{' '}
            <span className="font-semibold text-[rgb(var(--foreground))]">
              {space?.name ?? '…'}
            </span>{' '}
            (switch Spaces from the sidebar).
          </p>
        </div>
      </header>

      <div className="flex-1 overflow-auto p-8">
        <div className="mx-auto max-w-3xl space-y-6">
          {loading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="text-primary-500 h-6 w-6 animate-spin" />
            </div>
          ) : (
            servers.map((server) => (
              <Card key={server.id} data-testid={`builtin-server-${server.id}`}>
                <CardHeader>
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0">
                      <CardTitle className="flex items-center gap-2">
                        <span className="bg-primary-500/10 text-primary-500 flex h-8 w-8 items-center justify-center rounded-lg">
                          {SERVER_ICONS[server.id] ?? <Boxes className="h-5 w-5" />}
                        </span>
                        {server.name}
                        <span className="rounded-md border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
                          Built-in
                        </span>
                      </CardTitle>
                      <CardDescription className="mt-2">{server.description}</CardDescription>
                    </div>
                    <Switch
                      checked={server.enabled}
                      onCheckedChange={(v) => void toggleServer(server.id, v)}
                      data-testid={`builtin-server-toggle-${server.id}`}
                    />
                  </div>
                </CardHeader>
                <CardContent className="space-y-6">
                  <div>
                    <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
                      <Wrench className="h-3.5 w-3.5" />
                      Tools ({server.tools.length})
                    </div>
                    <div
                      className={`overflow-hidden rounded-xl border border-[rgb(var(--border))] transition-opacity ${
                        server.enabled ? '' : 'opacity-50'
                      }`}
                    >
                      <div className="divide-y divide-[rgb(var(--border-subtle))]">
                        {server.tools.map((t) => (
                          <div key={t.name} className="flex items-start gap-3 px-4 py-2.5">
                            {t.write ? (
                              <Pencil className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-500" />
                            ) : (
                              <Eye className="mt-0.5 h-4 w-4 flex-shrink-0 text-emerald-500" />
                            )}
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
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
                              disabled={!server.enabled}
                              onCheckedChange={(v) => void toggleTool(server.id, t.name, v)}
                              data-testid={`builtin-tool-toggle-${t.name}`}
                            />
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>

                  <MetaToolGrantsPanel />
                  <MetaToolAuditLog />
                </CardContent>
              </Card>
            ))
          )}

          {/* Framework preview — servers that slot into this same shell later. */}
          <div>
            <div className="mb-3 flex items-center gap-2">
              <Layers className="h-4 w-4 text-[rgb(var(--muted))]" />
              <h2 className="text-sm font-semibold text-[rgb(var(--foreground))]">
                More built-in servers
              </h2>
              <span className="text-xs text-[rgb(var(--muted))]">coming soon</span>
            </div>
            <div className="grid gap-4 sm:grid-cols-3">
              {COMING_SOON.map((s) => (
                <div
                  key={s.id}
                  className="rounded-xl border border-dashed border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-4 opacity-75"
                  data-testid={`builtin-server-coming-soon-${s.id}`}
                >
                  <div className="mb-2 flex items-center justify-between">
                    <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-[rgb(var(--background))] text-[rgb(var(--muted))]">
                      {s.icon}
                    </span>
                    <span className="rounded-md bg-[rgb(var(--background))] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
                      Soon
                    </span>
                  </div>
                  <div className="text-sm font-semibold">{s.name}</div>
                  <p className="mt-1 text-xs text-[rgb(var(--muted))]">{s.description}</p>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      <ToastContainer toasts={toasts} onClose={dismiss} />
    </div>
  );
}
