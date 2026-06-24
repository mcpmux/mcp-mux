import { FolderOpen, Globe, Monitor, Server, Wrench } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { StatTile } from '@/components/StatTile';
import { useViewSpace } from '@/stores';
import { spaceAccentColor } from '@/lib/spaceAccent';
import type { DashboardStats } from './dashboard.helpers';

interface DashboardStatCardsProps {
  stats: DashboardStats;
}

/** Accent palette for dashboard stat tiles. */
const ACCENTS = {
  servers: 'hsl(199 65% 52%)',
  featureSets: 'hsl(262 60% 58%)',
  clients: 'hsl(152 55% 45%)',
  workspaces: 'hsl(32 75% 52%)',
} as const;

/**
 * Top-row stat tiles — accent strip, tinted icons, nav on click.
 */
export function DashboardStatCards({ stats }: DashboardStatCardsProps) {
  const viewSpace = useViewSpace();
  const { t } = useTranslation(['dashboard', 'common']);

  return (
    <div
      className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5"
      data-testid="dashboard-stats-grid"
    >
      <StatTile
        testId="stat-servers"
        valueTestId="stat-servers-value"
        icon={Server}
        label={t('statCards.servers.title')}
        sub={t('statCards.servers.metric')}
        value={`${stats.connectedServers}/${stats.installedServers}`}
        accent={ACCENTS.servers}
        navTarget="servers"
        navHint={t('statCards.servers.ariaLabel')}
      />
      <StatTile
        testId="stat-featuresets"
        valueTestId="stat-featuresets-value"
        icon={Wrench}
        label={t('statCards.featuresets.title')}
        sub={t('statCards.featuresets.metric')}
        value={String(stats.featureSets)}
        accent={ACCENTS.featureSets}
        navTarget="featuresets"
        navHint={t('statCards.featuresets.ariaLabel')}
      />
      <StatTile
        testId="stat-clients"
        valueTestId="stat-clients-value"
        icon={Monitor}
        label={t('statCards.clients.title')}
        sub={t('statCards.clients.metric')}
        value={String(stats.clients)}
        accent={ACCENTS.clients}
        navTarget="clients"
        navHint={t('statCards.clients.ariaLabel')}
      />
      <StatTile
        testId="stat-active-space"
        valueTestId="stat-active-space-value"
        icon={Globe}
        label={t('statCards.space.title')}
        sub={t('statCards.space.total', { count: stats.spaces })}
        value={`${viewSpace?.icon ?? ''} ${viewSpace?.name || t('common:none')}`.trim()}
        accent={spaceAccentColor(viewSpace?.id)}
        navTarget="spaces"
        navHint={t('statCards.space.ariaLabel')}
      />
      <StatTile
        testId="stat-workspaces"
        valueTestId="stat-workspaces-value"
        icon={FolderOpen}
        label={t('statCards.workspaces.title')}
        sub={t('statCards.workspaces.metric')}
        value={String(stats.workspaceBindings)}
        accent={ACCENTS.workspaces}
        navTarget="workspaces"
        navHint={t('statCards.workspaces.ariaLabel')}
      />
    </div>
  );
}
