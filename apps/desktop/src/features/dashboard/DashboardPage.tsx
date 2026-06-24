import { useTranslation } from 'react-i18next';
import { PageHeader } from '@mcpmux/ui';
import { ConnectionCard } from '@/components/ConnectionCard';
import { DashboardQuickLinks } from './DashboardQuickLinks';
import { DashboardRecentActivity } from './DashboardRecentActivity';
import { DashboardServerHealth } from './DashboardServerHealth';
import { DashboardStatCards } from './DashboardStatCards';
import { GetStartedStrip } from './GetStartedStrip';
import { SetUpFolderCard } from './SetUpFolderCard';
import { useDashboardData } from './useDashboardData';

/**
 * Landing dashboard — gateway connection, stat cards, server health, and quick navigation.
 */
export function DashboardPage() {
  const { t } = useTranslation('dashboard');
  const { stats, attentionServers, isLoading } = useDashboardData();

  return (
    <div className="space-y-6" data-testid="dashboard-page">
      <PageHeader
        className="mb-0"
        title={t('page.title')}
        titleTestId="dashboard-title"
        subtitle={<span data-testid="dashboard-welcome">{t('page.welcome')}</span>}
      />

      {!isLoading && stats.installedServers === 0 && <GetStartedStrip />}

      <ConnectionCard />

      <SetUpFolderCard />

      <DashboardStatCards stats={stats} />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="space-y-4 lg:col-span-2">
          <DashboardServerHealth attentionServers={attentionServers} isLoading={isLoading} />
          <DashboardRecentActivity />
        </div>
        <DashboardQuickLinks />
      </div>
    </div>
  );
}
