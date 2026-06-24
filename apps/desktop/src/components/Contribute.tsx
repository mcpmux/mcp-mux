import { Bug, Github, Heart, Lightbulb, Package, SendHorizontal } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@mcpmux/ui';
import { CONTRIBUTE, openExternal } from '@/lib/contribute';

/**
 * Inline "Didn't find your server?" CTA used on empty search states in the
 * Registry page and the Add-Custom-Server flow.
 *
 * Ships two buttons side-by-side: **Request** (opens a pre-labelled GitHub
 * issue in mcp-servers with the search term in the title) and
 * **Contribute** (opens the mcp-servers CONTRIBUTING guide).
 */
export function RequestServerCTA({
  searchTerm,
  className,
}: {
  searchTerm?: string;
  className?: string;
}) {
  const { t } = useTranslation('settings');

  return (
    <div
      className={`rounded-xl border border-primary-200/60 dark:border-primary-800/40 bg-gradient-to-br from-primary-50/50 to-transparent dark:from-primary-900/10 p-4 flex flex-col sm:flex-row items-start sm:items-center gap-3 ${className ?? ''}`}
      data-testid="request-server-cta"
    >
      <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary-500/10 text-primary-600 dark:text-primary-300 flex-shrink-0">
        <Package className="h-4 w-4" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium">{t('contribute.requestCta.title')}</p>
        <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
          {searchTerm
            ? t('contribute.requestCta.descriptionWithSearch', { searchTerm })
            : t('contribute.requestCta.descriptionDefault')}
        </p>
      </div>
      <div className="flex items-center gap-2 flex-shrink-0">
        <Button
          variant="primary"
          size="sm"
          onClick={() => openExternal(CONTRIBUTE.requestServer(searchTerm))}
          data-testid="request-server-btn"
        >
          <SendHorizontal className="h-3 w-3 mr-1.5" />
          {t('contribute.requestCta.request')}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => openExternal(CONTRIBUTE.contributeServer)}
          data-testid="contribute-server-btn"
        >
          <Github className="h-3 w-3 mr-1.5" />
          {t('contribute.requestCta.contribute')}
        </Button>
      </div>
    </div>
  );
}

/**
 * A persistent "Contribute / Report" dropdown menu — the single global
 * affordance for: open GitHub repo, report a bug, request a feature, open
 * the server registry. Place wherever you want a friendly "help make mcpmux
 * better" call-to-action.
 */
export function ContributeMenu({
  variant = 'ghost',
  size = 'sm',
}: {
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md';
}) {
  const { t } = useTranslation('settings');

  const items = [
    {
      label: t('contribute.requestServer'),
      caption: t('contribute.requestServerDesc'),
      icon: Package,
      href: CONTRIBUTE.requestServer(),
    },
    {
      label: t('contribute.reportBug'),
      caption: t('contribute.reportBugDesc'),
      icon: Bug,
      href: CONTRIBUTE.bug,
    },
    {
      label: t('contribute.suggestFeature'),
      caption: t('contribute.suggestFeatureDesc'),
      icon: Lightbulb,
      href: CONTRIBUTE.featureRequest,
    },
    {
      label: t('contribute.openGithub'),
      caption: t('contribute.openGithubDesc'),
      icon: Github,
      href: CONTRIBUTE.repo,
    },
  ];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger data-testid="contribute-menu-trigger">
        <Button variant={variant} size={size} type="button">
          <Heart className="h-4 w-4 mr-1.5" />
          {t('contribute.menuLabel')}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72 p-1.5" data-testid="contribute-menu">
        {items.map((item) => (
          <DropdownMenuItem
            key={item.label}
            icon={item.icon}
            label={item.label}
            description={item.caption}
            onSelect={() => openExternal(item.href)}
          />
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
