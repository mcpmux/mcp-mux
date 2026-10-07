import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { RenameSpaceModal } from '@/features/spaces/RenameSpaceModal';

const space = {
  id: 'space-1',
  name: 'Work',
  icon: '💼',
  description: null,
  is_default: false,
  sort_order: 0,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

describe('RenameSpaceModal', () => {
  it('does not render when no Space is selected', () => {
    render(<RenameSpaceModal space={null} onClose={() => {}} onRename={async () => {}} />);

    expect(screen.queryByTestId('rename-space-modal-overlay')).toBeNull();
  });

  it('requires a changed, non-empty name', () => {
    render(<RenameSpaceModal space={space} onClose={() => {}} onRename={async () => {}} />);

    const submit = screen.getByTestId('rename-space-submit-btn');
    expect(submit).toBeDisabled();

    fireEvent.change(screen.getByTestId('rename-space-name-input'), { target: { value: '   ' } });
    expect(submit).toBeDisabled();

    fireEvent.change(screen.getByTestId('rename-space-name-input'), {
      target: { value: 'Client' },
    });
    expect(submit).toBeEnabled();
  });

  it('trims, saves, and closes the dialog', async () => {
    const onRename = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(<RenameSpaceModal space={space} onClose={onClose} onRename={onRename} />);

    fireEvent.change(screen.getByTestId('rename-space-name-input'), {
      target: { value: '  Client  ' },
    });
    fireEvent.click(screen.getByTestId('rename-space-submit-btn'));

    await waitFor(() => expect(onRename).toHaveBeenCalledWith(space, 'Client'));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
