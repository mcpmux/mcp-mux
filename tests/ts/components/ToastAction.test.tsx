import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Toast } from '../../../packages/ui/src/components/common/Toast';

describe('Toast with action', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should render action button when action is provided', () => {
    const onClose = vi.fn();
    const onClick = vi.fn();
    render(
      <Toast
        id="test-1"
        type="success"
        title="Server installed"
        message="Go enable it"
        duration={6000}
        action={{ label: 'Go to My Servers', onClick }}
        onClose={onClose}
      />
    );

    expect(screen.getByTestId('toast-action')).toBeInTheDocument();
    expect(screen.getByText('Go to My Servers')).toBeInTheDocument();
  });

  it('should not render action button when action is not provided', () => {
    const onClose = vi.fn();
    render(
      <Toast
        id="test-1"
        type="success"
        title="Done"
        duration={3000}
        onClose={onClose}
      />
    );

    expect(screen.queryByTestId('toast-action')).not.toBeInTheDocument();
  });

  it('should call action onClick and close toast when action button is clicked', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    const onClose = vi.fn();
    const onClick = vi.fn();

    render(
      <Toast
        id="test-1"
        type="success"
        title="Server installed"
        action={{ label: 'Go to My Servers', onClick }}
        duration={6000}
        onClose={onClose}
      />
    );

    await user.click(screen.getByTestId('toast-action'));

    expect(onClick).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledWith('test-1');
    vi.useFakeTimers();
  });
});
