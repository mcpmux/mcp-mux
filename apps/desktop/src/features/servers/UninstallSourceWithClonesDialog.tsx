import { AlertCircle } from 'lucide-react';

export interface CloneDependentSummary {
  server_id: string;
  server_name?: string | null;
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
  if (!open) {
    return null;
  }

  const dependentLabels = dependents.map(
    (dependent) => dependent.server_name ?? dependent.server_id
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
        className="mx-4 w-full max-w-md rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--card))] p-6 shadow-xl animate-in fade-in zoom-in-95 duration-200"
        onClick={(event) => event.stopPropagation()}
        data-testid="uninstall-clones-dialog"
      >
        <div className="mb-4 flex items-start gap-3">
          <div className="flex-shrink-0 rounded-full bg-amber-500/10 p-2">
            <AlertCircle className="h-5 w-5 text-amber-500" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Uninstall server with account clones?</h3>
            <p className="mt-2 text-sm text-[rgb(var(--muted))]">
              <span className="font-medium text-[rgb(var(--foreground))]">{sourceName}</span> has{' '}
              {dependents.length} account clone{dependents.length === 1 ? '' : 's'} in this space:{' '}
              <span className="font-medium text-[rgb(var(--foreground))]">{dependentList}</span>.
            </p>
            <p className="mt-2 text-sm text-[rgb(var(--muted))]">
              Uninstalling the source leaves clones installed and working. You can also remove
              everything at once.
            </p>
          </div>
        </div>
        <div className="flex flex-wrap justify-end gap-3">
          <button
            onClick={onCancel}
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface-active))] px-4 py-2 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
            data-testid="uninstall-clones-cancel"
          >
            Cancel
          </button>
          <button
            onClick={onUninstallSourceOnly}
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface-active))] px-4 py-2 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
            data-testid="uninstall-clones-source-only"
          >
            Uninstall source only
          </button>
          <button
            onClick={onUninstallAll}
            className="rounded-lg bg-red-600 px-4 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-red-700"
            data-testid="uninstall-clones-all"
          >
            Uninstall all ({totalCount})
          </button>
        </div>
      </div>
    </div>
  );
}
