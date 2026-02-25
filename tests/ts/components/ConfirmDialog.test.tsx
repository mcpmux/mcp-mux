import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ConfirmDialog, useConfirm } from '../../../packages/ui/src/components/common/ConfirmDialog';

describe('ConfirmDialog', () => {
  it('should not render when closed', () => {
    render(
      <ConfirmDialog
        open={false}
        title="Delete"
        message="Are you sure?"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );

    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });

  it('should render when open', () => {
    render(
      <ConfirmDialog
        open={true}
        title="Delete item"
        message="This cannot be undone."
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );

    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
    expect(screen.getByText('Delete item')).toBeInTheDocument();
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
  });

  it('should show custom confirm label', () => {
    render(
      <ConfirmDialog
        open={true}
        title="Delete"
        message="Sure?"
        confirmLabel="Yes, delete"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );

    expect(screen.getByTestId('confirm-dialog-confirm')).toHaveTextContent('Yes, delete');
  });

  it('should show danger icon for danger variant', () => {
    render(
      <ConfirmDialog
        open={true}
        title="Delete"
        message="Sure?"
        variant="danger"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );

    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
  });

  it('should call onConfirm when confirm is clicked', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();

    render(
      <ConfirmDialog
        open={true}
        title="Delete"
        message="Sure?"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );

    await user.click(screen.getByTestId('confirm-dialog-confirm'));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it('should call onCancel when cancel is clicked', async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();

    render(
      <ConfirmDialog
        open={true}
        title="Delete"
        message="Sure?"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />
    );

    await user.click(screen.getByTestId('confirm-dialog-cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('should call onCancel when overlay is clicked', async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();

    render(
      <ConfirmDialog
        open={true}
        title="Delete"
        message="Sure?"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />
    );

    await user.click(screen.getByTestId('confirm-dialog-overlay'));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe('useConfirm', () => {
  function TestComponent({ onResult }: { onResult: (v: boolean) => void }) {
    const { confirm, ConfirmDialogElement } = useConfirm();

    return (
      <div>
        <button
          data-testid="trigger"
          onClick={async () => {
            const result = await confirm({
              title: 'Confirm action',
              message: 'Do you want to proceed?',
              confirmLabel: 'Proceed',
              variant: 'danger',
            });
            onResult(result);
          }}
        >
          Open
        </button>
        {ConfirmDialogElement}
      </div>
    );
  }

  it('should resolve true when confirmed', async () => {
    const user = userEvent.setup();
    const onResult = vi.fn();

    render(<TestComponent onResult={onResult} />);

    // Dialog should not be visible initially
    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();

    // Open dialog
    await user.click(screen.getByTestId('trigger'));

    // Dialog should be visible
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
    expect(screen.getByText('Confirm action')).toBeInTheDocument();
    expect(screen.getByText('Do you want to proceed?')).toBeInTheDocument();
    expect(screen.getByTestId('confirm-dialog-confirm')).toHaveTextContent('Proceed');

    // Click confirm
    await user.click(screen.getByTestId('confirm-dialog-confirm'));

    expect(onResult).toHaveBeenCalledWith(true);
    // Dialog should close
    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });

  it('should resolve false when cancelled', async () => {
    const user = userEvent.setup();
    const onResult = vi.fn();

    render(<TestComponent onResult={onResult} />);

    await user.click(screen.getByTestId('trigger'));
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();

    await user.click(screen.getByTestId('confirm-dialog-cancel'));

    expect(onResult).toHaveBeenCalledWith(false);
    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });

  it('should resolve false when overlay is clicked', async () => {
    const user = userEvent.setup();
    const onResult = vi.fn();

    render(<TestComponent onResult={onResult} />);

    await user.click(screen.getByTestId('trigger'));
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();

    await user.click(screen.getByTestId('confirm-dialog-overlay'));

    expect(onResult).toHaveBeenCalledWith(false);
    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });
});
