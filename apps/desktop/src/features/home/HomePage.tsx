/**
 * Home — the control-room landing page.
 *
 * Extracted from App.tsx (was `DashboardView`) so the shell stays a pure
 * layout/router and Home can grow into the superapp heartbeat screen
 * (activity feed, agent inbox) without touching the shell.
 *
 * Today it shows: the canonical connection surface (ConnectionCard) and a
 * row of stat tiles that double as navigation — every tile is a button into
 * the page that manages what it counts.
 */
import { useEffect, useState, useCallback } from 'react';
import {
  Server,
  Wrench,
  Monitor,
  Globe,
  ArrowUpRight,
  Compass,
  ArrowRight,
  FolderPlus,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { PageHeader } from '@mcpmux/ui';
import { ConnectionCard } from '@/components/ConnectionCard';
import { useGatewayEvents, useServerStatusEvents, useDomainEvents } from '@/hooks/useDomainEvents';
import { useViewSpace, useNavigateTo, useSetPendingWorkspaceNew } from '@/stores';
import type { NavItem } from '@/stores/types';
import { spaceAccentColor } from '@/lib/spaceAccent';

interface StatTileProps {
  testId: string;
  valueTestId: string;
  icon: LucideIcon;
  label: string;
  sub: string;
  value: string;
  /** Solid accent for the strip + icon tint. */
  accent: string;
  navTarget: NavItem;
  navHint: string;
}

function StatTile({
  testId,
  valueTestId,
  icon: Icon,
  label,
  sub,
  value,
  accent,
  navTarget,
  navHint,
}: StatTileProps) {
  const navigateTo = useNavigateTo();
  return (
    <button
      type="button"
      onClick={() => navigateTo(navTarget)}
      title={navHint}
      data-testid={testId}
      className="group relative overflow-hidden rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:border-[rgb(var(--border))] hover:shadow-md"
    >
      {/* Solid accent strip — the app-wide status/identity language. */}
      <span
        aria-hidden
        className="absolute inset-y-0 left-0 w-1"
        style={{ backgroundColor: accent }}
      />
      <div className="flex items-start justify-between gap-2 pl-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-medium text-[rgb(var(--muted))]">
            <span
              className="flex h-7 w-7 items-center justify-center rounded-lg"
              style={{ backgroundColor: `color-mix(in srgb, ${accent} 14%, transparent)` }}
            >
              <Icon className="h-4 w-4" style={{ color: accent }} />
            </span>
            {label}
          </div>
          <div
            className="mt-2 truncate text-3xl font-bold tracking-tight"
            data-testid={valueTestId}
          >
            {value}
          </div>
          <div className="mt-0.5 text-xs text-[rgb(var(--muted))]">{sub}</div>
        </div>
        <ArrowUpRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted-foreground))] opacity-0 transition-opacity duration-150 group-hover:opacity-100" />
      </div>
    </button>
  );
}

/**
 * Three-step journey shown only while the Space has zero installed servers —
 * it walks a newcomer from empty to "my AI app has tools" and disappears
 * forever after the first install.
 */
function GetStartedStrip() {
  const navigateTo = useNavigateTo();
  const steps = [
    {
      n: 1,
      icon: Compass,
      title: 'Pick your first tools',
      desc: 'Browse the registry and install a server in one click.',
      cta: 'Open Discover',
      nav: 'registry' as NavItem,
    },
    {
      n: 2,
      icon: Server,
      title: 'Enable it',
      desc: 'Turn the server on so the gateway can serve its tools.',
      cta: 'Open Tools',
      nav: 'servers' as NavItem,
    },
    {
      n: 3,
      icon: Monitor,
      title: 'Connect an AI app',
      desc: 'Point Cursor, Claude, or VS Code at your gateway below.',
      cta: 'See Apps',
      nav: 'clients' as NavItem,
    },
  ];
  return (
    <div
      className="overflow-hidden rounded-xl border border-[rgb(var(--primary))]/25 bg-[rgb(var(--primary))]/5"
      data-testid="home-get-started"
    >
      <div className="grid grid-cols-1 divide-y divide-[rgb(var(--primary))]/15 md:grid-cols-3 md:divide-x md:divide-y-0">
        {steps.map((s) => (
          <button
            key={s.n}
            type="button"
            onClick={() => navigateTo(s.nav)}
            className="group flex items-start gap-3 p-4 text-left transition-colors hover:bg-[rgb(var(--primary))]/10"
          >
            <span className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--primary))]/15 text-sm font-bold text-[rgb(var(--primary))]">
              {s.n}
            </span>
            <span className="min-w-0">
              <span className="flex items-center gap-1.5 text-sm font-semibold">
                <s.icon className="h-3.5 w-3.5 text-[rgb(var(--primary))]" />
                {s.title}
              </span>
              <span className="mt-0.5 block text-xs leading-relaxed text-[rgb(var(--muted))]">
                {s.desc}
              </span>
              <span className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-[rgb(var(--primary))]">
                {s.cta}
                <ArrowRight className="h-3 w-3 transition-transform group-hover:translate-x-0.5" />
              </span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * Per-folder setup entry point. The ConnectionCard above connects an app to
 * the gateway globally; this routes into the Workspaces walkthrough to map a
 * specific project and write its per-folder config.
 */
function SetUpFolderCard() {
  const navigateTo = useNavigateTo();
  const openWizard = useSetPendingWorkspaceNew();
  return (
    <button
      type="button"
      onClick={() => {
        openWizard(true);
        navigateTo('workspaces');
      }}
      data-testid="home-setup-folder"
      className="group flex w-full items-center gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:border-[rgb(var(--border))] hover:shadow-md"
    >
      <span className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--primary))]/12 text-[rgb(var(--primary))]">
        <FolderPlus className="h-5 w-5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-semibold">Set up a folder</span>
        <span className="block text-xs text-[rgb(var(--muted))]">
          Map a project to its tools and connect your apps to it — even ones that don&apos;t report
          the folder.
        </span>
      </span>
      <ArrowRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] transition-transform group-hover:translate-x-0.5" />
    </button>
  );
}

export function HomePage() {
  const [stats, setStats] = useState({
    installedServers: 0,
    connectedServers: 0,
    clients: 0,
    featureSets: 0,
  });
  const [statsLoaded, setStatsLoaded] = useState(false);
  const viewSpace = useViewSpace();

  const loadStats = useCallback(async () => {
    try {
      const [clients, featureSets, gateway, installedServers] = await Promise.all([
        import('@/lib/api/clients').then((m) => m.listClients()),
        import('@/lib/api/featureSets').then((m) =>
          viewSpace?.id ? m.listFeatureSetsBySpace(viewSpace.id) : m.listFeatureSets()
        ),
        import('@/lib/api/gateway').then((m) => m.getGatewayStatus(viewSpace?.id)),
        import('@/lib/api/registry').then((m) => m.listInstalledServers(viewSpace?.id)),
      ]);
      setStats({
        installedServers: installedServers.length,
        connectedServers: gateway.connected_backends,
        clients: clients.length,
        featureSets: featureSets.length,
      });
      setStatsLoaded(true);
    } catch (e) {
      console.error('Failed to load home stats:', e);
    }
  }, [viewSpace?.id]);

  // Load on mount and when the viewed Space changes.
  useEffect(() => {
    loadStats();
  }, [loadStats]);

  // Keep `Tools: X/Y` honest across gateway start/stop and backend churn.
  // ConnectionCard owns the actual running/URL UI.
  useGatewayEvents((payload) => {
    if (payload.action === 'started') {
      loadStats();
    } else if (payload.action === 'stopped') {
      setStats((prev) => ({ ...prev, connectedServers: 0 }));
    }
  });

  useServerStatusEvents((payload) => {
    if (payload.status === 'connected' || payload.status === 'disconnected') {
      loadStats();
    }
  });

  // Keep the FeatureSets + Clients tiles live when those change anywhere —
  // e.g. an MCP client composing a FeatureSet via `mcpmux_manage_feature_set`,
  // or a new app authenticating. Without this the counts go stale until a
  // Space switch or reload.
  const { subscribe } = useDomainEvents();
  useEffect(() => {
    const unsubs = [
      subscribe('feature-set-changed', () => void loadStats()),
      subscribe('client-changed', () => void loadStats()),
      subscribe('server-changed', () => void loadStats()),
    ];
    return () => unsubs.forEach((u) => u());
  }, [subscribe, loadStats]);

  return (
    <div className="space-y-6">
      <PageHeader
        title="Home"
        subtitle="Your AI control plane at a glance — one gateway, every app, your rules."
      />

      {/* First-steps journey — only until the first server is installed. */}
      {statsLoaded && stats.installedServers === 0 && <GetStartedStrip />}

      {/* Canonical connection surface — owns URL, Start/Stop, IDE grid,
          pending-approval nudge. */}
      <ConnectionCard />

      {/* Per-folder setup — opens the Workspaces walkthrough. */}
      <SetUpFolderCard />

      {/* Stat tiles — each is a shortcut into the page that manages it. */}
      <div
        className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4"
        data-testid="dashboard-stats-grid"
      >
        <StatTile
          testId="stat-servers"
          valueTestId="stat-servers-value"
          icon={Server}
          label="Tools"
          sub="Connected / Installed"
          value={`${stats.connectedServers}/${stats.installedServers}`}
          accent="hsl(199 65% 52%)"
          navTarget="servers"
          navHint="Manage your MCP servers"
        />
        <StatTile
          testId="stat-featuresets"
          valueTestId="stat-featuresets-value"
          icon={Wrench}
          label="FeatureSets"
          sub="Curated tool bundles"
          value={String(stats.featureSets)}
          accent="hsl(262 60% 58%)"
          navTarget="featuresets"
          navHint="Curate which tools go where"
        />
        <StatTile
          testId="stat-clients"
          valueTestId="stat-clients-value"
          icon={Monitor}
          label="Apps"
          sub="Connected AI apps"
          value={String(stats.clients)}
          accent="hsl(152 55% 45%)"
          navTarget="clients"
          navHint="See the AI apps using your gateway"
        />
        <StatTile
          testId="stat-active-space"
          valueTestId="stat-active-space-value"
          icon={Globe}
          label="Space"
          sub="Currently viewing"
          value={`${viewSpace?.icon ?? ''} ${viewSpace?.name || 'None'}`.trim()}
          accent={spaceAccentColor(viewSpace?.id)}
          navTarget="spaces"
          navHint="Switch or manage Spaces"
        />
      </div>
    </div>
  );
}
