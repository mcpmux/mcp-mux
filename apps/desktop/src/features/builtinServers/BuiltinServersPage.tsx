/**
 * Built-in Servers page.
 *
 * Home for the MCP servers McpMux ships itself — distinct from the servers the
 * user installs (those live under "My Servers"). Each built-in server can be
 * enabled/disabled; when on, its tools appear to connected MCP clients
 * alongside the user's own tools.
 *
 * Today there's one concrete built-in server — "Tool Optimization" (the
 * `mcpmux_*` self-management toolset). Memory / Skills / Plugins are scaffolded
 * as "coming soon" so the shape of the framework is visible. Per-workspace
 * enable/disable + per-tool toggles land on top of this in a later pass.
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
} from 'lucide-react';
import { getMetaToolsEnabled, setMetaToolsEnabled } from '@/lib/api/metaTools';
import { MetaToolAuditLog, MetaToolGrantsPanel } from '@/features/metaTools';

/** A tool the Tool Optimization server exposes — display metadata only. */
interface BuiltinTool {
  name: string;
  description: string;
  write: boolean;
}

/**
 * The `mcpmux_*` tools, mirrored from the gateway's meta-tool registry. Kept
 * here for display; the gateway is the source of truth for what's actually
 * advertised. Writes require a native approval before they run.
 */
const TOOL_OPTIMIZATION_TOOLS: BuiltinTool[] = [
  {
    name: 'mcpmux_list_all_tools',
    description: 'Browse every tool available in the resolved Space, unfiltered.',
    write: false,
  },
  {
    name: 'mcpmux_list_feature_sets',
    description: 'See the feature sets defined in the Space.',
    write: false,
  },
  {
    name: 'mcpmux_create_feature_set',
    description: 'Build a focused feature set from chosen tools.',
    write: true,
  },
  {
    name: 'mcpmux_bind_current_workspace',
    description: 'Map the current folder to a feature set so it persists.',
    write: true,
  },
];

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
    description: 'Search a catalog of skills and pull the ones you want into a workspace.',
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

  const [enabled, setEnabled] = useState(true);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getMetaToolsEnabled()
      .then(setEnabled)
      .catch((e) => console.error('Failed to load meta_tools_enabled', e))
      .finally(() => setLoading(false));
  }, []);

  // Live-sync if the switch is flipped elsewhere (the gateway forwards
  // `meta-tools-changed` after a toggle); keep this page honest without a
  // manual refresh.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/event').then(({ listen }) => {
      void listen<{ enabled: boolean }>('meta-tools-changed', (e) => {
        setEnabled(e.payload.enabled);
      }).then((fn) => {
        unlisten = fn;
      });
    });
    return () => unlisten?.();
  }, []);

  const handleToggle = async (next: boolean) => {
    const previous = enabled;
    setEnabled(next);
    try {
      await setMetaToolsEnabled(next);
      success(
        next ? 'Tool Optimization enabled' : 'Tool Optimization disabled',
        next
          ? 'Connected clients now see the mcpmux_* tools.'
          : 'The mcpmux_* tools are hidden from connected clients.'
      );
    } catch (e) {
      setEnabled(previous);
      error('Failed to save', e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="flex h-full flex-col" data-testid="builtin-servers-page">
      <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
        <div className="mx-auto max-w-[2000px]">
          <h1 className="text-3xl font-bold">Built-in Servers</h1>
          <p className="mt-2 max-w-2xl text-base text-[rgb(var(--muted))]">
            MCP servers that ship with McpMux — separate from the ones you install under{' '}
            <span className="font-medium text-[rgb(var(--foreground))]">My Servers</span>. Turn a
            server on and its tools appear to every connected client alongside your own.
          </p>
        </div>
      </header>

      <div className="flex-1 overflow-auto p-8">
        <div className="mx-auto max-w-3xl space-y-6">
          {/* Tool Optimization — the mcpmux_* self-management server */}
          <Card data-testid="builtin-server-tool-optimization">
            <CardHeader>
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <CardTitle className="flex items-center gap-2">
                    <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary-500/10 text-primary-500">
                      <Sparkles className="h-5 w-5" />
                    </span>
                    Tool Optimization
                    <span className="rounded-md border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
                      Built-in
                    </span>
                  </CardTitle>
                  <CardDescription className="mt-2">
                    Lets the AI keep its own toolset lean: it can browse every available tool,
                    assemble a focused feature set, and pin it to the current folder — each with
                    your approval. Reads are silent; writes pop a native approval dialog.
                  </CardDescription>
                </div>
                <Switch
                  checked={enabled}
                  onCheckedChange={handleToggle}
                  disabled={loading}
                  data-testid="meta-tools-enabled-switch"
                />
              </div>
            </CardHeader>
            <CardContent className="space-y-6">
              {/* Tools this server exposes */}
              <div>
                <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
                  <Wrench className="h-3.5 w-3.5" />
                  Tools ({TOOL_OPTIMIZATION_TOOLS.length})
                </div>
                <div
                  className={`overflow-hidden rounded-xl border border-[rgb(var(--border))] transition-opacity ${
                    enabled ? '' : 'opacity-50'
                  }`}
                >
                  <div className="divide-y divide-[rgb(var(--border-subtle))]">
                    {TOOL_OPTIMIZATION_TOOLS.map((t) => (
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
                          <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">{t.description}</p>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              <MetaToolGrantsPanel />
              <MetaToolAuditLog />
            </CardContent>
          </Card>

          {/* Framework preview — servers that slot into this same shell later. */}
          <div>
            <div className="mb-3 flex items-center gap-2">
              <Boxes className="h-4 w-4 text-[rgb(var(--muted))]" />
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
