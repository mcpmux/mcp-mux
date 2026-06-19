/**
 * Spaces — "Base directories" modal.
 *
 * The modal must show the space's existing base dirs, let you remove one
 * easily, add a folder as a clearly-optional action, and close via "Done"
 * without being forced to add anything.
 *
 * `@mcpmux/ui` is aliased to the real source in vitest.config, so the real
 * Button / toast render.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { listMock, addMock, removeMock } = vi.hoisted(() => ({
  listMock: vi.fn(),
  addMock: vi.fn(),
  removeMock: vi.fn(),
}));

vi.mock('@/lib/api/spaces', () => ({
  listSpaceBaseDirs: listMock,
  addSpaceBaseDir: addMock,
  removeSpaceBaseDir: removeMock,
}));

import { SpaceBaseDirsModal } from '@/features/spaces/SpaceBaseDirsModal';

const SPACE = {
  id: 's1',
  name: 'Work',
  icon: '💼',
  description: null,
  is_default: false,
  sort_order: 0,
  created_at: '',
  updated_at: '',
};

function dir(id: string, path: string) {
  return { id, space_id: 's1', path, created_at: '' };
}

describe('SpaceBaseDirsModal', () => {
  beforeEach(() => {
    listMock.mockReset();
    addMock.mockReset();
    removeMock.mockReset();
  });

  it('lists the space’s existing base directories', async () => {
    listMock.mockResolvedValue([dir('d1', '/work/a'), dir('d2', '/work/b')]);
    render(<SpaceBaseDirsModal space={SPACE} onClose={() => {}} />);

    expect(await screen.findByText('/work/a')).toBeTruthy();
    expect(screen.getByText('/work/b')).toBeTruthy();
  });

  it('removes a directory with one click', async () => {
    const user = userEvent.setup();
    removeMock.mockResolvedValue(undefined);
    listMock.mockResolvedValue([dir('d1', '/work/a')]);
    render(<SpaceBaseDirsModal space={SPACE} onClose={() => {}} />);

    await screen.findByText('/work/a');
    await user.click(screen.getByTestId('remove-base-dir-d1'));

    await waitFor(() => expect(removeMock).toHaveBeenCalledWith('d1'));
    await waitFor(() => expect(screen.queryByText('/work/a')).toBeNull());
  });

  it('closes via "Done" without adding anything', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    listMock.mockResolvedValue([]);
    render(<SpaceBaseDirsModal space={SPACE} onClose={onClose} />);

    await screen.findByTestId('add-base-dir-btn'); // loaded (empty)
    await user.click(screen.getByTestId('space-base-dirs-done'));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(addMock).not.toHaveBeenCalled();
  });

  it('renders nothing when no space is selected', () => {
    const { container } = render(<SpaceBaseDirsModal space={null} onClose={() => {}} />);
    expect(container).toBeEmptyDOMElement();
    expect(listMock).not.toHaveBeenCalled();
  });
});
