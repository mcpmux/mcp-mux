/**
 * Source Badge component for displaying server installation source.
 */

import type { InstallationSource } from '@/types/registry';

interface SourceBadgeProps {
  source: InstallationSource | undefined;
  className?: string;
}

/**
 * Badge showing where a server was installed from.
 * 
 * - Registry: Blue badge - installed from official/bundled registry
 * - Config File: Green badge - synced from user's JSON config file
 * - Manual: Gray badge - manually entered via UI
 */
export function SourceBadge({ source, className = '' }: SourceBadgeProps) {
  if (!source) {
    return null;
  }

  switch (source.type) {
    case 'registry':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200 ${className}`}
          title="Installed from registry"
        >
          Registry
        </span>
      );

    case 'user_config':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200 ${className}`}
          title={`From config: ${source.file_path}`}
        >
          Config File
        </span>
      );

    case 'manual_entry':
      return (
        <span
          className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-200 ${className}`}
          title="Manually added"
        >
          Manual
        </span>
      );

    default:
      return null;
  }
}

/**
 * Get the appropriate uninstall action label based on source.
 */
export function getUninstallLabel(source: InstallationSource | undefined): string {
  if (!source) {
    return 'Uninstall';
  }

  switch (source.type) {
    case 'user_config':
      return 'Remove from Config';
    case 'manual_entry':
      return 'Remove';
    case 'registry':
    default:
      return 'Uninstall';
  }
}

/**
 * Get confirmation message for uninstalling based on source.
 */
export function getUninstallConfirmMessage(
  serverName: string,
  source: InstallationSource | undefined
): string {
  if (source?.type === 'user_config') {
    return `This will remove "${serverName}" from your config file. You can re-add it by editing the config file.`;
  }
  return `Are you sure you want to uninstall "${serverName}"? You can reinstall it from the registry.`;
}
