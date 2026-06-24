import type { KeyboardEvent } from 'react';
import {
  Globe,
  Monitor,
  FolderOpen,
  Server,
  Wrench,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@mcpmux/ui';
import { useNavigateTo, useViewSpace } from '@/stores';
import type { DashboardStats } from './dashboard.helpers';

interface DashboardStatCardsProps {
  stats: DashboardStats;
}

const STAT_CARD_CLASS =
  'cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/50';

/**
 * Navigate when the user activates a stat card via click or keyboard.
 */
function activateStatCard(
  event: KeyboardEvent<HTMLDivElement>,
  navigate: () => void
) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    navigate();
  }
}

/**
 * Top-row stat cards with descriptions and deep links into sidebar destinations.
 */
export function DashboardStatCards({ stats }: DashboardStatCardsProps) {
  const navigateTo = useNavigateTo();
  const viewSpace = useViewSpace();
  const { t } = useTranslation(['dashboard', 'common']);

  return (
    <div
      className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-5"
      data-testid="dashboard-stats-grid"
    >
      <Card
        className={STAT_CARD_CLASS}
        data-testid="stat-servers"
        role="button"
        tabIndex={0}
        aria-label={t('statCards.servers.ariaLabel')}
        onClick={() => navigateTo('servers')}
        onKeyDown={(event) => activateStatCard(event, () => navigateTo('servers'))}
      >
        <CardHeader className="mb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <Server className="h-5 w-5 text-primary-500" />
            {t('statCards.servers.title')}
          </CardTitle>
          <CardDescription>{t('statCards.servers.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-3xl font-bold" data-testid="stat-servers-value">
            {stats.connectedServers}/{stats.installedServers}
          </div>
          <div className="text-sm text-[rgb(var(--muted))]">{t('statCards.servers.metric')}</div>
        </CardContent>
      </Card>

      <Card
        className={STAT_CARD_CLASS}
        data-testid="stat-featuresets"
        role="button"
        tabIndex={0}
        aria-label={t('statCards.featuresets.ariaLabel')}
        onClick={() => navigateTo('featuresets')}
        onKeyDown={(event) => activateStatCard(event, () => navigateTo('featuresets'))}
      >
        <CardHeader className="mb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <Wrench className="h-5 w-5 text-primary-500" />
            {t('statCards.featuresets.title')}
          </CardTitle>
          <CardDescription>{t('statCards.featuresets.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-3xl font-bold" data-testid="stat-featuresets-value">
            {stats.featureSets}
          </div>
          <div className="text-sm text-[rgb(var(--muted))]">{t('statCards.featuresets.metric')}</div>
        </CardContent>
      </Card>

      <Card
        className={STAT_CARD_CLASS}
        data-testid="stat-clients"
        role="button"
        tabIndex={0}
        aria-label={t('statCards.clients.ariaLabel')}
        onClick={() => navigateTo('clients')}
        onKeyDown={(event) => activateStatCard(event, () => navigateTo('clients'))}
      >
        <CardHeader className="mb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <Monitor className="h-5 w-5 text-primary-500" />
            {t('statCards.clients.title')}
          </CardTitle>
          <CardDescription>{t('statCards.clients.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-3xl font-bold" data-testid="stat-clients-value">
            {stats.clients}
          </div>
          <div className="text-sm text-[rgb(var(--muted))]">{t('statCards.clients.metric')}</div>
        </CardContent>
      </Card>

      <Card
        className={STAT_CARD_CLASS}
        data-testid="stat-active-space"
        role="button"
        tabIndex={0}
        aria-label={t('statCards.space.ariaLabel')}
        onClick={() => navigateTo('spaces')}
        onKeyDown={(event) => activateStatCard(event, () => navigateTo('spaces'))}
      >
        <CardHeader className="mb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <Globe className="h-5 w-5 text-primary-500" />
            {t('statCards.space.title')}
          </CardTitle>
          <CardDescription>{t('statCards.space.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="truncate text-xl font-bold" data-testid="stat-active-space-value">
            {viewSpace?.icon} {viewSpace?.name || t('common:none')}
          </div>
          <div className="text-sm text-[rgb(var(--muted))]">
            {t('statCards.space.total', { count: stats.spaces })}
          </div>
        </CardContent>
      </Card>

      <Card
        className={STAT_CARD_CLASS}
        data-testid="stat-workspaces"
        role="button"
        tabIndex={0}
        aria-label={t('statCards.workspaces.ariaLabel')}
        onClick={() => navigateTo('workspaces')}
        onKeyDown={(event) => activateStatCard(event, () => navigateTo('workspaces'))}
      >
        <CardHeader className="mb-2">
          <CardTitle className="flex items-center gap-2 text-base">
            <FolderOpen className="h-5 w-5 text-primary-500" />
            {t('statCards.workspaces.title')}
          </CardTitle>
          <CardDescription>{t('statCards.workspaces.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-3xl font-bold" data-testid="stat-workspaces-value">
            {stats.workspaceBindings}
          </div>
          <div className="text-sm text-[rgb(var(--muted))]">{t('statCards.workspaces.metric')}</div>
        </CardContent>
      </Card>
    </div>
  );
}
