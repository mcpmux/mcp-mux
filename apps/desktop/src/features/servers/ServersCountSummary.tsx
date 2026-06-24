import { useTranslation } from 'react-i18next';
import { HoverTooltip } from '@mcpmux/ui';
import {
  describeServerCountSummary,
  formatServerCountSummary,
  type ServerCountSummary,
} from './servers-page.helpers';

interface ServersCountSummaryProps {
  summary: ServerCountSummary;
}

/**
 * Inline installed-server counts beside the My Servers title, with hover breakdown.
 */
export function ServersCountSummary({ summary }: ServersCountSummaryProps) {
  const { t } = useTranslation('servers');

  if (summary.installed === 0) {
    return null;
  }

  return (
    <HoverTooltip
      title={t('countSummary.title')}
      lines={describeServerCountSummary(t, summary)}
      data-testid="servers-count-tooltip"
      className="flex-shrink min-w-0"
    >
      <p
        className="text-sm text-[rgb(var(--muted))] truncate cursor-default"
        data-testid="servers-count-summary"
      >
        {formatServerCountSummary(t, summary)}
      </p>
    </HoverTooltip>
  );
}
