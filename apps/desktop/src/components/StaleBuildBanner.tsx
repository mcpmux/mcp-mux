import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, X } from 'lucide-react';
import { getBuildInfo } from '@/lib/api/app';
import { isTauri } from '@/lib/backend/shell';

/**
 * Warns when the web-admin static bundle was built from a different commit than
 * the running backend — the usual cause of a stale or incomplete dashboard.
 */
export function StaleBuildBanner() {
  const { t } = useTranslation('common');
  const [isStale, setIsStale] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (import.meta.env.DEV || isTauri()) {
      return;
    }

    const spaSha = import.meta.env.VITE_BUILD_GIT_SHA;
    if (!spaSha) {
      return;
    }

    getBuildInfo()
      .then(({ git_sha }) => {
        if (git_sha && git_sha !== spaSha) {
          setIsStale(true);
        }
      })
      .catch(() => {});
  }, []);

  if (!isStale || dismissed) {
    return null;
  }

  return (
    <div
      className="flex items-start justify-between gap-3 px-4 py-2.5 border-b border-amber-300/60 dark:border-amber-700/60 bg-amber-50 dark:bg-amber-900/20 text-sm"
      data-testid="stale-build-banner"
      role="alert"
    >
      <div className="flex items-start gap-2 min-w-0">
        <AlertTriangle className="h-4 w-4 text-amber-600 dark:text-amber-400 mt-0.5 flex-shrink-0" />
        <div className="min-w-0">
          <p className="font-semibold text-amber-800 dark:text-amber-200">
            {t('staleBuild.title')}
          </p>
          <p className="text-amber-700 dark:text-amber-300 mt-0.5">
            {t('staleBuild.descriptionBefore')}{' '}
            <code className="font-mono text-xs bg-amber-100/80 dark:bg-amber-900/40 px-1 py-0.5 rounded">
              pnpm build:web:admin
            </code>{' '}
            {t('staleBuild.descriptionAfter')}
          </p>
        </div>
      </div>
      <button
        type="button"
        onClick={() => setDismissed(true)}
        className="text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] transition-colors flex-shrink-0"
        aria-label={t('staleBuild.dismissAria')}
        data-testid="dismiss-stale-build-banner"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
