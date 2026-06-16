/**
 * Shared Create-Space dialog: name + icon picker, used by both the Spaces page
 * and the sidebar SpaceSwitcher.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';

const { mockCreateSpace, mockAddSpace } = vi.hoisted(() => ({
  mockCreateSpace: vi.fn(),
  mockAddSpace: vi.fn(),
}));

vi.mock('@/lib/api/spaces', () => ({ createSpace: mockCreateSpace }));
vi.mock('@/stores', () => ({
  useAppStore: (selector: (s: { addSpace: typeof mockAddSpace }) => unknown) =>
    selector({ addSpace: mockAddSpace }),
}));

import { CreateSpaceModal } from '@/features/spaces/CreateSpaceModal';

describe('CreateSpaceModal', () => {
  beforeEach(() => {
    mockCreateSpace.mockReset();
    mockAddSpace.mockReset();
  });

  it('renders nothing when closed', () => {
    render(<CreateSpaceModal open={false} onClose={() => {}} />);
    expect(screen.queryByTestId('create-space-modal')).toBeNull();
  });

  it('disables Create until a name is entered', () => {
    render(<CreateSpaceModal open onClose={() => {}} />);
    expect(screen.getByTestId('create-space-submit-btn')).toBeDisabled();
    fireEvent.change(screen.getByTestId('create-space-name-input'), {
      target: { value: 'Work' },
    });
    expect(screen.getByTestId('create-space-submit-btn')).toBeEnabled();
  });

  it('selecting an icon updates the preview and the custom field', () => {
    render(<CreateSpaceModal open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId('create-space-icon-🚀'));
    expect(screen.getByTestId('create-space-preview-icon')).toHaveTextContent('🚀');
    expect(screen.getByTestId('create-space-icon-custom')).toHaveValue('🚀');
  });

  it('creates with the chosen name + icon, then fires onCreated and onClose', async () => {
    const space = { id: 's1', name: 'Work', icon: '💼', is_default: false, description: null };
    mockCreateSpace.mockResolvedValue(space);
    const onClose = vi.fn();
    const onCreated = vi.fn();
    render(<CreateSpaceModal open onClose={onClose} onCreated={onCreated} />);

    fireEvent.change(screen.getByTestId('create-space-name-input'), {
      target: { value: '  Work  ' },
    });
    fireEvent.click(screen.getByTestId('create-space-icon-💼'));
    fireEvent.click(screen.getByTestId('create-space-submit-btn'));

    await waitFor(() => expect(mockCreateSpace).toHaveBeenCalledWith('Work', '💼'));
    expect(mockAddSpace).toHaveBeenCalledWith(space);
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(space));
    expect(onClose).toHaveBeenCalled();
  });
});
