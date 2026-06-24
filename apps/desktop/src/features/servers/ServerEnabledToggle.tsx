import { useTranslation } from 'react-i18next';
import { Switch } from '@mcpmux/ui';

interface ServerEnabledToggleProps {
  serverId: string;
  enabled: boolean;
  isLoading: boolean;
  disabled?: boolean;
  onToggle: (enabled: boolean) => void;
}

/**
 * Labeled enable/disable control for an installed server row.
 */
export function ServerEnabledToggle({
  serverId,
  enabled,
  isLoading,
  disabled = false,
  onToggle,
}: ServerEnabledToggleProps) {
  const { t } = useTranslation('servers');
  const label = isLoading
    ? enabled
      ? t('enabledToggle.disabling')
      : t('enabledToggle.enabling')
    : enabled
      ? t('enabledToggle.enabled')
      : t('enabledToggle.disabled');

  return (
    <div className="flex items-center gap-2">
      <span className="text-sm text-[rgb(var(--muted))] whitespace-nowrap">{label}</span>
      <Switch
        checked={enabled}
        onCheckedChange={onToggle}
        disabled={disabled || isLoading}
        data-testid={enabled ? `disable-server-${serverId}` : `enable-server-${serverId}`}
      />
    </div>
  );
}
