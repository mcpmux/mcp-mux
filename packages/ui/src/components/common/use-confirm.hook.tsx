import { useCallback, useRef, useState } from 'react';
import { ConfirmDialog } from './ConfirmDialog';
import type { ConfirmDialogState } from './ConfirmDialog';

/**
 * Hook that provides a promise-based confirm dialog.
 *
 * Usage:
 * ```tsx
 * const { confirm, ConfirmDialogElement } = useConfirm();
 *
 * const handleDelete = async () => {
 *   if (!await confirm({ title: 'Delete?', message: 'This cannot be undone.' })) return;
 *   // proceed with delete
 * };
 *
 * return <>{ConfirmDialogElement}</>;
 * ```
 */
export function useConfirm() {
  const [state, setState] = useState<ConfirmDialogState & { key: number }>({
    open: false,
    title: '',
    message: '',
    key: 0,
  });
  const resolveRef = useRef<((value: boolean) => void) | null>(null);

  const confirm = useCallback(
    (options: Omit<ConfirmDialogState, 'open'>) => {
      return new Promise<boolean>((resolve) => {
        resolveRef.current = resolve;
        setState((prev) => ({ ...options, open: true, key: prev.key + 1 }));
      });
    },
    []
  );

  const handleConfirm = useCallback(() => {
    setState((prev) => ({ ...prev, open: false }));
    resolveRef.current?.(true);
    resolveRef.current = null;
  }, []);

  const handleCancel = useCallback(() => {
    setState((prev) => ({ ...prev, open: false }));
    resolveRef.current?.(false);
    resolveRef.current = null;
  }, []);

  const { key: dialogKey, ...dialogState } = state;
  const ConfirmDialogElement = (
    <ConfirmDialog
      key={dialogKey}
      {...dialogState}
      onConfirm={handleConfirm}
      onCancel={handleCancel}
    />
  );

  return { confirm, ConfirmDialogElement };
}
