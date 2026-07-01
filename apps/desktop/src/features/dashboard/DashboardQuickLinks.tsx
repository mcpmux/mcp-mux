import type { ReactNode } from 'react';
import { FolderOpen, Globe, Monitor, Search, Settings, ShoppingBasket } from 'lucide-react';
import { McpNavIcon } from '@/components/McpNavIcon';
import { useTranslation } from 'react-i18next';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@mcpmux/ui';
import { useNavigate } from '@/hooks/use-navigate.hook';
import type { NavItem } from '@/stores/types';

type QuickLinkConfig = {
  nav: NavItem;
  labelKey: 'myServers' | 'search' | 'spaces' | 'bundles' | 'projects' | 'clients' | 'settings';
  descriptionKey:
    | 'quickLinks.descriptions.servers'
    | 'quickLinks.descriptions.registry'
    | 'quickLinks.descriptions.spaces'
    | 'quickLinks.descriptions.featuresets'
    | 'quickLinks.descriptions.workspaces'
    | 'quickLinks.descriptions.clients'
    | 'quickLinks.descriptions.settings';
  icon: ReactNode;
  testId: string;
};

const QUICK_LINK_CONFIG: QuickLinkConfig[] = [
  {
    nav: 'servers',
    labelKey: 'myServers',
    descriptionKey: 'quickLinks.descriptions.servers',
    icon: <McpNavIcon className="h-4 w-4" />,
    testId: 'quick-link-servers',
  },
  {
    nav: 'registry',
    labelKey: 'search',
    descriptionKey: 'quickLinks.descriptions.registry',
    icon: <Search className="h-4 w-4" />,
    testId: 'quick-link-discover',
  },
  {
    nav: 'spaces',
    labelKey: 'spaces',
    descriptionKey: 'quickLinks.descriptions.spaces',
    icon: <Globe className="h-4 w-4" />,
    testId: 'quick-link-spaces',
  },
  {
    nav: 'featuresets',
    labelKey: 'bundles',
    descriptionKey: 'quickLinks.descriptions.featuresets',
    icon: <ShoppingBasket className="h-4 w-4" />,
    testId: 'quick-link-featuresets',
  },
  {
    nav: 'workspaces',
    labelKey: 'projects',
    descriptionKey: 'quickLinks.descriptions.workspaces',
    icon: <FolderOpen className="h-4 w-4" />,
    testId: 'quick-link-workspaces',
  },
  {
    nav: 'clients',
    labelKey: 'clients',
    descriptionKey: 'quickLinks.descriptions.clients',
    icon: <Monitor className="h-4 w-4" />,
    testId: 'quick-link-clients',
  },
  {
    nav: 'settings',
    labelKey: 'settings',
    descriptionKey: 'quickLinks.descriptions.settings',
    icon: <Settings className="h-4 w-4" />,
    testId: 'quick-link-settings',
  },
];

/**
 * Compact navigation grid covering every sidebar destination except Dashboard.
 */
export function DashboardQuickLinks() {
  const navigate = useNavigate();
  const { t: tNav } = useTranslation('nav');
  const { t: tDashboard } = useTranslation('dashboard');

  return (
    <Card data-testid="dashboard-quick-links">
      <CardHeader>
        <CardTitle className="text-base">{tDashboard('quickLinks.title')}</CardTitle>
        <CardDescription>{tDashboard('quickLinks.description')}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 gap-2">
          {QUICK_LINK_CONFIG.map((link) => (
            <button
              key={link.nav}
              type="button"
              onClick={() => navigate(link.nav)}
              data-testid={link.testId}
              className="flex items-start gap-3 rounded-lg border border-[rgb(var(--border-subtle))] px-3 py-2.5 text-left transition-colors hover:border-[rgb(var(--primary))/30] hover:bg-[rgb(var(--surface-hover))]"
            >
              <span className="mt-0.5 text-[rgb(var(--primary))]">{link.icon}</span>
              <span className="min-w-0">
                <span className="block text-sm font-medium">{tNav(link.labelKey)}</span>
                <span className="block truncate text-xs text-[rgb(var(--muted))]">
                  {tDashboard(link.descriptionKey)}
                </span>
              </span>
            </button>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}
