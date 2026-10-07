/**
 * Server logs are stored per Space (`logs/<space>/<server>/current.log`), so
 * every read, clear and path lookup has to name the Space being viewed rather
 * than fall back to the default one.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { ServerLogViewer } from '@/components/ServerLogViewer';

// The viewer scrolls to the newest entry shortly after each load; jsdom
// has no scrolling, so without this the timer throws after the assertions.
Element.prototype.scrollTo = vi.fn();

const SERVER_ID = 'com.meta-business';
const WORK_SPACE = '02233890-a6ee-4b1e-aea3-fca8e3b4c09d';
const OTHER_SPACE = '7d1c2f4e-5b6a-4c3d-8e9f-0a1b2c3d4e5f';

const entry = {
  timestamp: '2026-10-07T13:00:00Z',
  level: 'info',
  source: 'stderr',
  message: 'listening on stdio',
};

function renderViewer(spaceId: string) {
  return render(
    <ServerLogViewer
      serverId={SERVER_ID}
      serverName="Meta Business"
      spaceId={spaceId}
      onClose={() => {}}
    />
  );
}

describe('ServerLogViewer', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_server_logs') return [entry];
      if (cmd === 'get_server_log_file') return `/logs/${WORK_SPACE}/${SERVER_ID}/current.log`;
      return undefined;
    });
  });

  it('reads the logs of the Space it is given', async () => {
    renderViewer(WORK_SPACE);

    expect(await screen.findByText('listening on stdio')).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('get_server_logs', {
      serverId: SERVER_ID,
      spaceId: WORK_SPACE,
      limit: 500,
      levelFilter: undefined,
    });
  });

  it('reloads from the new Space when the Space changes', async () => {
    const { rerender } = renderViewer(WORK_SPACE);
    await screen.findByText('listening on stdio');

    rerender(
      <ServerLogViewer
        serverId={SERVER_ID}
        serverName="Meta Business"
        spaceId={OTHER_SPACE}
        onClose={() => {}}
      />
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        'get_server_logs',
        expect.objectContaining({ serverId: SERVER_ID, spaceId: OTHER_SPACE })
      )
    );
  });

  it('copies the log file path of the Space it is given', async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    // user-event installs its own clipboard stub, so ours goes in after it.
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    });
    renderViewer(WORK_SPACE);
    await screen.findByText('listening on stdio');

    await user.click(screen.getByTitle('Open log file in external editor'));

    expect(invoke).toHaveBeenCalledWith('get_server_log_file', {
      serverId: SERVER_ID,
      spaceId: WORK_SPACE,
    });
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(`/logs/${WORK_SPACE}/${SERVER_ID}/current.log`)
    );
  });

  it('clears the logs of the Space it is given', async () => {
    const user = userEvent.setup();
    renderViewer(WORK_SPACE);
    await screen.findByText('listening on stdio');

    await user.click(screen.getByTitle('Clear all logs'));
    await user.click(await screen.findByTestId('confirm-dialog-confirm'));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('clear_server_logs', {
        serverId: SERVER_ID,
        spaceId: WORK_SPACE,
      })
    );
    expect(screen.queryByText('listening on stdio')).not.toBeInTheDocument();
  });
});
