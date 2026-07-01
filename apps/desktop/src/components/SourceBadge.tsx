/**
 * Source Badge component for displaying server installation source.
 */

import { useTranslation } from 'react-i18next';
import type { InstallationSource } from '@/types/registry';

interface SourceBadgeProps {
  source: InstallationSource | undefined;
  /** Source server ID when this install is a clone (display-only lineage). */
  clonedFrom?: string | null;
  className?: string;
}

/**
 * Badge showing where a server was installed from.
 */
export function SourceBadge({ source, clonedFrom, className = '' }: SourceBadgeProps) {
  const { t } = useTranslation('common');

  if (clonedFrom) {
    return (
      <span
        className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-indigo-100 text-indigo-800 dark:bg-indigo-900 dark:text-indigo-200 ${className}`}
        title={t('sourceBadge.cloneTitle', { serverId: clonedFrom })}
        data-testid="source-badge-clone"
      >
        {t('sourceBadge.cloneLabel', { serverId: clonedFrom })}
      </span>
    );
  }

  if (!source) {
    return null;
  }

  switch (source.type) {
    case 'registry':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200 ${className}`}
          title={t('sourceBadge.registryTitle')}
        >
          {t('sourceBadge.registry')}
        </span>
      );

    case 'user_config':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200 ${className}`}
          title={t('sourceBadge.configFileTitle', { path: source.file_path })}
        >
          {t('sourceBadge.configFile')}
        </span>
      );

    case 'manual_entry':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-200 ${className}`}
          title={t('sourceBadge.manualTitle')}
        >
          {t('sourceBadge.manual')}
        </span>
      );

    default:
      return null;
  }
}
