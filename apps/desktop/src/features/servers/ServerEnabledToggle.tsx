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
  const label = isLoading ? (enabled ? 'Disabling…' : 'Enabling…') : enabled ? 'Enabled' : 'Disabled';

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
