import { useState, useCallback } from 'react';
import { ToastProps, ToastType, ToastAction } from '../components/common/Toast';

export interface ToastOptions {
  title: string;
  message?: string;
  type?: ToastType;
  duration?: number;
  action?: ToastAction;
}

export function useToast() {
  const [toasts, setToasts] = useState<ToastProps[]>([]);

  const showToast = useCallback((options: ToastOptions) => {
    const id = `toast-${Date.now()}-${Math.random()}`;
    const toast: ToastProps = {
      id,
      type: options.type || 'info',
      title: options.title,
      message: options.message,
      duration: options.duration ?? 3000,
      action: options.action,
      onClose: (toastId: string) => {
        setToasts((prev) => prev.filter((t) => t.id !== toastId));
      },
    };

    setToasts((prev) => [...prev, toast]);
    return id;
  }, []);

  const success = useCallback(
    (title: string, message?: string, options?: number | { duration?: number; action?: ToastAction }) => {
      const opts = typeof options === 'number' ? { duration: options } : options;
      return showToast({ title, message, type: 'success', duration: opts?.duration, action: opts?.action });
    },
    [showToast]
  );

  const error = useCallback(
    (title: string, message?: string, duration?: number) => {
      return showToast({ title, message, type: 'error', duration });
    },
    [showToast]
  );

  const warning = useCallback(
    (title: string, message?: string, duration?: number) => {
      return showToast({ title, message, type: 'warning', duration });
    },
    [showToast]
  );

  const info = useCallback(
    (title: string, message?: string, duration?: number) => {
      return showToast({ title, message, type: 'info', duration });
    },
    [showToast]
  );

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  return {
    toasts,
    showToast,
    success,
    error,
    warning,
    info,
    dismiss,
  };
}
