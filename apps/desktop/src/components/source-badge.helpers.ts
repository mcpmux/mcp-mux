import type { TFunction } from 'i18next';
import type { InstallationSource } from '@/types/registry';

/**
 * Get the appropriate uninstall action label based on source.
 */
export function getUninstallLabel(
  t: TFunction<'common'>,
  source: InstallationSource | undefined
): string {
  if (!source) {
    return t('sourceBadge.uninstall');
  }

  switch (source.type) {
    case 'user_config':
      return t('sourceBadge.removeFromConfig');
    case 'manual_entry':
      return t('sourceBadge.remove');
    case 'registry':
    default:
      return t('sourceBadge.uninstall');
  }
}

/**
 * Get confirmation message for uninstalling based on source.
 */
export function getUninstallConfirmMessage(
  t: TFunction<'common'>,
  serverName: string,
  source: InstallationSource | undefined
): string {
  if (source?.type === 'user_config') {
    return t('sourceBadge.confirmUserConfig', { serverName });
  }
  return t('sourceBadge.confirmDefault', { serverName });
}
