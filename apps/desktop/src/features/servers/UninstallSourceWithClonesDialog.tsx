import { useTranslation } from 'react-i18next';
import { AlertCircle } from 'lucide-react';
import { resolveInstalledDisplayName } from './server-display-name.helpers';

export interface CloneDependentSummary {
  server_id: string;
  server_name?: string | null;
  display_name_override?: string | null;
}

interface UninstallSourceWithClonesDialogProps {
  open: boolean;
  sourceName: string;
  dependents: CloneDependentSummary[];
  onCancel: () => void;
  onUninstallSourceOnly: () => void;
  onUninstallAll: () => void;
}

/**
 * Warn when uninstalling a source server that still has account clones in the same space.
 */
export function UninstallSourceWithClonesDialog({
  open,
  sourceName,
  dependents,
  onCancel,
  onUninstallSourceOnly,
  onUninstallAll,
}: UninstallSourceWithClonesDialogProps) {
  const { t } = useTranslation(['servers', 'common']);

  if (!open) {
    return null;
  }

  const dependentLabels = dependents.map((dependent) =>
    resolveInstalledDisplayName({
      server_id: dependent.server_id,
      server_name: dependent.server_name ?? null,
      display_name_override: dependent.display_name_override ?? null,
    })
  );
  const dependentList = dependentLabels.join(', ');
  const totalCount = dependents.length + 1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onCancel}
      data-testid="uninstall-clones-dialog-overlay"
    >
      <div
        className="animate-in fade-in zoom-in-95 mx-4 w-full max-w-md rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--card))] p-6 shadow-xl duration-200"
        onClick={(event) => event.stopPropagation()}
        data-testid="uninstall-clones-dialog"
      >
        <div className="mb-4 flex items-start gap-3">
          <div className="flex-shrink-0 rounded-full bg-amber-500/10 p-2">
            <AlertCircle className="h-5 w-5 text-amber-500" />
          </div>
          <div>
            <h3 className="text-base font-semibold">{t('uninstallClones.title')}</h3>
            <p className="mt-2 text-sm text-[rgb(var(--muted))]">
              {t('uninstallClones.description', {
                count: dependents.length,
                sourceName,
                dependentList,
              })}
            </p>
            <p className="mt-2 text-sm text-[rgb(var(--muted))]">{t('uninstallClones.hint')}</p>
          </div>
        </div>
        <div className="flex flex-wrap justify-end gap-3">
          <button
            onClick={onCancel}
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface-active))] px-4 py-2 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
            data-testid="uninstall-clones-cancel"
          >
            {t('common:actions.cancel')}
          </button>
          <button
            onClick={onUninstallSourceOnly}
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface-active))] px-4 py-2 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
            data-testid="uninstall-clones-source-only"
          >
            {t('uninstallClones.sourceOnly')}
          </button>
          <button
            onClick={onUninstallAll}
            className="rounded-lg bg-red-600 px-4 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-red-700"
            data-testid="uninstall-clones-all"
          >
            {t('uninstallClones.uninstallAll', { count: totalCount })}
          </button>
        </div>
      </div>
    </div>
  );
}
