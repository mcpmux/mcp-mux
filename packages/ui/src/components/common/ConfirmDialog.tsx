import { AlertCircle } from 'lucide-react';

export interface ConfirmDialogState {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: 'danger' | 'default';
}

export interface ConfirmDialogProps extends ConfirmDialogState {
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  variant = 'default',
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  if (!open) return null;

  const isDanger = variant === 'danger';

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onCancel}
      data-testid="confirm-dialog-overlay"
    >
      <div
        className="mx-4 w-full max-w-sm rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--card))] p-6 shadow-xl animate-in fade-in zoom-in-95 duration-200"
        onClick={(e) => e.stopPropagation()}
        data-testid="confirm-dialog"
      >
        <div className="flex items-start gap-3 mb-4">
          {isDanger && (
            <div className="rounded-full bg-red-500/10 p-2 flex-shrink-0">
              <AlertCircle className="h-5 w-5 text-red-500" />
            </div>
          )}
          <div>
            <h3 className="font-semibold text-base" data-testid="confirm-dialog-title">
              {title}
            </h3>
            <p className="text-sm text-[rgb(var(--muted))] mt-1">{message}</p>
          </div>
        </div>
        <div className="flex gap-3 justify-end">
          <button
            onClick={onCancel}
            className="px-4 py-2 text-sm font-medium rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface-active))] text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
            data-testid="confirm-dialog-cancel"
          >
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            className={`px-4 py-2 text-sm font-medium rounded-lg shadow-sm transition-colors ${
              isDanger
                ? 'bg-red-600 text-white hover:bg-red-700'
                : 'bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))]'
            }`}
            data-testid="confirm-dialog-confirm"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

