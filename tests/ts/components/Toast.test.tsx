import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Toast, ToastContainer } from '../../../packages/ui/src/components/common/Toast';

describe('Toast', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should render success toast', () => {
    const onClose = vi.fn();
    render(
      <Toast
        id="test-1"
        type="success"
        title="Success!"
        message="Operation completed"
        duration={3000}
        onClose={onClose}
      />
    );

    expect(screen.getByText('Success!')).toBeInTheDocument();
    expect(screen.getByText('Operation completed')).toBeInTheDocument();
    expect(screen.getByTestId('toast-success')).toBeInTheDocument();
  });

  it('should render error toast', () => {
    const onClose = vi.fn();
    render(
      <Toast
        id="test-1"
        type="error"
        title="Error!"
        duration={3000}
        onClose={onClose}
      />
    );

    expect(screen.getByText('Error!')).toBeInTheDocument();
    expect(screen.getByTestId('toast-error')).toBeInTheDocument();
  });

  it('should render toast without message', () => {
    const onClose = vi.fn();
    render(
      <Toast
        id="test-1"
        type="info"
        title="Info"
        duration={3000}
        onClose={onClose}
      />
    );

    expect(screen.getByText('Info')).toBeInTheDocument();
    expect(screen.queryByText('Operation completed')).not.toBeInTheDocument();
  });

  it('should call onClose when close button is clicked', async () => {
    const user = userEvent.setup({ delay: null });
    const onClose = vi.fn();
    
    render(
      <Toast
        id="test-1"
        type="info"
        title="Info"
        duration={3000}
        onClose={onClose}
      />
    );

    const closeButton = screen.getByTestId('toast-close');
    await user.click(closeButton);

    expect(onClose).toHaveBeenCalledWith('test-1');
  });

  it('should auto-dismiss after duration', () => {
    const onClose = vi.fn();
    
    render(
      <Toast
        id="test-1"
        type="info"
        title="Info"
        duration={3000}
        onClose={onClose}
      />
    );

    expect(onClose).not.toHaveBeenCalled();

    vi.advanceTimersByTime(3000);

    expect(onClose).toHaveBeenCalledWith('test-1');
  });

  it('should not auto-dismiss when duration is 0', () => {
    const onClose = vi.fn();
    
    render(
      <Toast
        id="test-1"
        type="info"
        title="Info"
        duration={0}
        onClose={onClose}
      />
    );

    vi.advanceTimersByTime(10000);

    expect(onClose).not.toHaveBeenCalled();
  });
});

describe('ToastContainer', () => {
  it('should render multiple toasts', () => {
    const onClose = vi.fn();
    const toasts = [
      {
        id: 'toast-1',
        type: 'success' as const,
        title: 'Success 1',
        duration: 3000,
        onClose,
      },
      {
        id: 'toast-2',
        type: 'error' as const,
        title: 'Error 1',
        duration: 3000,
        onClose,
      },
    ];

    render(<ToastContainer toasts={toasts} onClose={onClose} />);

    expect(screen.getByText('Success 1')).toBeInTheDocument();
    expect(screen.getByText('Error 1')).toBeInTheDocument();
    expect(screen.getByTestId('toast-container')).toBeInTheDocument();
  });

  it('should render empty container when no toasts', () => {
    const onClose = vi.fn();
    
    render(<ToastContainer toasts={[]} onClose={onClose} />);

    const container = screen.getByTestId('toast-container');
    expect(container).toBeInTheDocument();
    expect(container.children).toHaveLength(0);
  });
});
