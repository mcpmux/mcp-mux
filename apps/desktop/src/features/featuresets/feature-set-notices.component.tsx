import { useTranslation } from 'react-i18next';
import { AlertTriangle, Zap } from 'lucide-react';
import { Switch } from '@mcpmux/ui';
import { MuxPromptCode } from '@/components/MuxPrompt';

interface AutoIncludeCardProps {
  isAutoInclude: boolean;
  isDisabled: boolean;
  onToggle: (enabled: boolean) => void;
}

/**
 * Switch card for auto mode: the set grants every server's tools, including ones added later.
 */
export function FeatureSetAutoIncludeCard({
  isAutoInclude,
  isDisabled,
  onToggle,
}: AutoIncludeCardProps) {
  const { t } = useTranslation('featuresets');

  return (
    <div
      className={`rounded-xl border-2 p-4 ${
        isAutoInclude
          ? 'border-emerald-300 bg-emerald-50/60 dark:border-emerald-700/60 dark:bg-emerald-900/15'
          : 'border-[rgb(var(--border))] bg-[rgb(var(--background))]'
      }`}
      data-testid="featureset-auto-card"
    >
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <Zap
            className={`mt-0.5 h-5 w-5 flex-shrink-0 ${
              isAutoInclude ? 'text-emerald-600 dark:text-emerald-400' : 'text-[rgb(var(--muted))]'
            }`}
          />
          <div>
            <p className="text-sm font-semibold">{t('auto.title')}</p>
            <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">
              {isAutoInclude ? t('auto.onHint') : t('auto.offHint')}
            </p>
          </div>
        </div>
        <Switch
          checked={isAutoInclude}
          onCheckedChange={onToggle}
          disabled={isDisabled}
          data-testid="featureset-auto-switch"
        />
      </div>
    </div>
  );
}

interface ToolCountWarningProps {
  toolCount: number;
  threshold: number;
}

/**
 * Warning shown when the selection serves more tools than AI apps handle well.
 */
export function FeatureSetToolCountWarning({ toolCount, threshold }: ToolCountWarningProps) {
  const { t } = useTranslation('featuresets');

  return (
    <div
      className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 dark:border-amber-700/60 dark:bg-amber-900/20"
      data-testid="featureset-panel-tools-warning"
    >
      <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold text-amber-900 dark:text-amber-100">
          {t('auto.warningTitle', { count: toolCount, threshold })}
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-amber-800 dark:text-amber-200">
          {t('auto.warningBody')}
        </p>
        <div className="mt-2">
          <MuxPromptCode testId="featureset-panel-tools-warning-copy" />
        </div>
      </div>
    </div>
  );
}
