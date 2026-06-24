import { useTranslation } from 'react-i18next';
import { AlertCircle, Loader2 } from 'lucide-react';
import { useBuildStamp, type UseBuildStampResult } from './use-build-stamp.hook';
import type { BuildStampRow } from '@/lib/build-info.helpers';

type BuildStampContext = 'desktop' | 'web-admin';

/**
 * Load build stamp data and render the panel (for parents that do not already hold stamp state).
 */
export function BuildStampPanel({ context }: { context: BuildStampContext }) {
  const stamp = useBuildStamp();
  return <BuildStampPanelContent context={context} stamp={stamp} />;
}

/**
 * Presentational build stamp panel — accepts preloaded stamp data.
 */
export function BuildStampPanelContent({
  context,
  stamp,
}: {
  context: BuildStampContext;
  stamp: UseBuildStampResult;
}) {
  const { t } = useTranslation('settings');
  const { backendRows, spaRows, spaSha, backendSha, hasMismatch, loading, error } = stamp;

  if (loading) {
    return (
      <div
        className="flex items-center gap-2 text-xs text-[rgb(var(--muted))] mt-2"
        data-testid="build-stamp-panel"
      >
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        {t('buildStamp.loading')}
      </div>
    );
  }

  if (error) {
    return (
      <p className="text-xs text-[rgb(var(--muted))] mt-2" data-testid="build-stamp-panel">
        {t('buildStamp.unavailable')}
      </p>
    );
  }

  return (
    <div className="mt-3 space-y-3" data-testid="build-stamp-panel">
      <BuildStampRowGroup rows={backendRows} heading={t('buildStamp.buildHeading')} />

      {hasMismatch ? (
        <div
          className="flex items-start gap-2 p-3 rounded-lg border border-amber-300 dark:border-amber-700/60 bg-amber-50 dark:bg-amber-900/20 text-xs"
          data-testid="build-stamp-mismatch"
          role="alert"
        >
          <AlertCircle className="h-4 w-4 text-amber-600 dark:text-amber-400 mt-0.5 flex-shrink-0" />
          <div>
            <p className="font-semibold text-amber-800 dark:text-amber-200">
              {context === 'web-admin'
                ? t('buildStamp.webAdminMismatchTitle')
                : t('buildStamp.desktopMismatchTitle')}
            </p>
            <p className="text-amber-700 dark:text-amber-300 mt-0.5">
              {context === 'web-admin' ? (
                <>
                  {t('buildStamp.webAdminMismatchDescBefore', { spaSha, backendSha })}{' '}
                  <code className="font-mono text-[11px] bg-amber-100/80 dark:bg-amber-900/40 px-1 py-0.5 rounded">
                    {t('buildStamp.webAdminBuildCommand')}
                  </code>{' '}
                  {t('buildStamp.webAdminMismatchDescAfter')}
                </>
              ) : (
                t('buildStamp.desktopMismatchDesc', { spaSha, backendSha })
              )}
            </p>
          </div>
        </div>
      ) : null}

      {hasMismatch ? (
        <BuildStampRowGroup
          rows={spaRows}
          heading={t('buildStamp.uiBundleHeading')}
          testIdPrefix="spa-"
        />
      ) : null}
    </div>
  );
}

/**
 * Render a group of labeled build stamp rows.
 */
function BuildStampRowGroup({
  rows,
  heading,
  testIdPrefix = '',
}: {
  rows: BuildStampRow[];
  heading?: string;
  testIdPrefix?: string;
}) {
  return (
    <div className="space-y-2">
      {heading ? (
        <p className="text-xs font-medium text-[rgb(var(--muted))] uppercase tracking-wide">
          {heading}
        </p>
      ) : null}
      {rows.map((row) => (
        <div key={`${testIdPrefix}${row.testId}`} className="grid grid-cols-[5.5rem_1fr] gap-x-3 gap-y-0.5">
          <span className="text-xs text-[rgb(var(--muted))]">{row.label}</span>
          <span
            className={`text-sm ${row.mono ? 'font-mono' : ''} text-[rgb(var(--foreground))]`}
            data-testid={`${testIdPrefix}${row.testId}`}
          >
            {row.value}
          </span>
        </div>
      ))}
    </div>
  );
}
