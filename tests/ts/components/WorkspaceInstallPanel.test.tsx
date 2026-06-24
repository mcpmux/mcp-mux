/**
 * Workspaces — "Connect apps to this folder" install panel.
 *
 * The panel must: list the supported clients, write the selected clients'
 * configs via `install_workspace_mcp_config` with this folder's path as the
 * `X-Mcpmux-Workspace` header (carried server-side), copy a per-client snippet,
 * and surface (and be able to flip) the system-wide auth toggle inline.
 *
 * `@mcpmux/ui` is aliased to real source in vitest.config, so the real Button
 * renders.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const {
  listClientsMock,
  installMock,
  snippetMock,
  getAuthMock,
  gatewayStatusMock,
  navigateMock,
  setSectionMock,
} = vi.hoisted(() => ({
  listClientsMock: vi.fn(),
  installMock: vi.fn(),
  snippetMock: vi.fn(),
  getAuthMock: vi.fn(),
  gatewayStatusMock: vi.fn(),
  navigateMock: vi.fn(),
  setSectionMock: vi.fn(),
}));

vi.mock('@/lib/api/workspaceInstall', () => ({
  listWorkspaceInstallClients: listClientsMock,
  installWorkspaceMcpConfig: installMock,
  generateWorkspaceConfigSnippet: snippetMock,
  getGatewayAuthDisabled: getAuthMock,
}));

vi.mock('@/lib/api/gateway', () => ({
  getGatewayStatus: gatewayStatusMock,
}));

vi.mock('@/stores', () => ({
  useNavigateTo: () => navigateMock,
  useSetPendingSettingsSection: () => setSectionMock,
}));

import { WorkspaceInstallPanel } from '@/features/workspaces/WorkspaceInstallPanel';

const CLIENTS = [
  { id: 'cursor', label: 'Cursor', config_path: '.cursor/mcp.json' },
  { id: 'claude-code', label: 'Claude Code', config_path: '.mcp.json' },
  { id: 'vscode', label: 'VS Code / Copilot', config_path: '.vscode/mcp.json' },
  { id: 'opencode', label: 'opencode', config_path: 'opencode.json' },
  { id: 'zed', label: 'Zed', config_path: '.zed/settings.json' },
];

const ROOT = process.platform === 'win32' ? 'd:\\proj\\app' : '/proj/app';

describe('WorkspaceInstallPanel', () => {
  beforeEach(() => {
    localStorage.clear();
    listClientsMock.mockReset().mockResolvedValue(CLIENTS);
    installMock.mockReset();
    snippetMock.mockReset();
    getAuthMock.mockReset().mockResolvedValue(true);
    navigateMock.mockReset();
    setSectionMock.mockReset();
    gatewayStatusMock
      .mockReset()
      .mockResolvedValue({ running: true, url: 'http://localhost:45818' });
  });

  it('lists every supported client', async () => {
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);
    expect(await screen.findByText('Cursor')).toBeTruthy();
    for (const c of CLIENTS) {
      expect(screen.getByTestId(`workspace-install-client-${c.id}`)).toBeTruthy();
    }
  });

  it('installs the default-selected clients with the gateway /mcp url', async () => {
    const user = userEvent.setup();
    installMock.mockResolvedValue([
      { client: 'cursor', label: 'Cursor', path: '/p/.cursor/mcp.json', action: 'created', backed_up: null, error: null },
    ]);
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);

    const btn = await screen.findByTestId('workspace-install-button');
    await user.click(btn);

    await waitFor(() => expect(installMock).toHaveBeenCalledTimes(1));
    const arg = installMock.mock.calls[0][0];
    expect(arg.workspaceRoot).toBe(ROOT);
    expect(arg.serverUrl).toBe('http://localhost:45818/mcp');
    // Defaults to the common three.
    expect(arg.clients).toEqual(['cursor', 'claude-code', 'vscode']);
    // Result row is shown.
    expect(await screen.findByTestId('workspace-install-results')).toBeTruthy();
  });

  it('remembers the previous client selection across renders', async () => {
    const user = userEvent.setup();
    installMock.mockResolvedValue([]);

    // First mount: deselect the defaults down to just opencode, then install
    // (which persists the selection).
    const first = render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);
    await screen.findByTestId('workspace-install-client-cursor');
    for (const id of ['cursor', 'claude-code', 'vscode']) {
      await user.click(screen.getByTestId(`workspace-install-client-${id}`));
    }
    await user.click(screen.getByTestId('workspace-install-client-opencode'));
    await user.click(screen.getByTestId('workspace-install-button'));
    await waitFor(() => expect(installMock).toHaveBeenCalled());
    expect(installMock.mock.calls[0][0].clients).toEqual(['opencode']);
    first.unmount();

    // Second mount: the remembered selection (opencode only) is restored, not
    // the big-three default.
    installMock.mockClear();
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);
    await screen.findByTestId('workspace-install-button');
    await user.click(screen.getByTestId('workspace-install-button'));
    await waitFor(() => expect(installMock).toHaveBeenCalled());
    expect(installMock.mock.calls[0][0].clients).toEqual(['opencode']);
  });

  it('shows the auth nudge and routes to Settings (no inline disable)', async () => {
    const user = userEvent.setup();
    getAuthMock.mockResolvedValue(false); // auth currently required
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);

    // Auth is application-wide: the panel links to Settings instead of
    // flipping it inline.
    const openSettings = await screen.findByTestId('workspace-install-open-auth-settings');
    await user.click(openSettings);
    expect(setSectionMock).toHaveBeenCalledWith('security');
    expect(navigateMock).toHaveBeenCalledWith('settings');
  });

  it('copies a client snippet to the clipboard', async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    snippetMock.mockResolvedValue({
      client: 'cursor',
      label: 'Cursor',
      config_path: '.cursor/mcp.json',
      content: '{ "mcpServers": { "mcpmux": {} } }',
    });
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);

    const copyBtn = await screen.findByTestId('workspace-install-copy-cursor');
    await user.click(copyBtn);

    await waitFor(() =>
      expect(snippetMock).toHaveBeenCalledWith(
        expect.objectContaining({ client: 'cursor', workspaceRoot: ROOT })
      )
    );
    await waitFor(() => expect(writeText).toHaveBeenCalled());
  });

  it('blocks install until the gateway is running', async () => {
    gatewayStatusMock.mockResolvedValue({ running: false, url: null });
    render(<WorkspaceInstallPanel workspaceRoot={ROOT} />);
    const btn = await screen.findByTestId('workspace-install-button');
    await waitFor(() => expect(btn).toHaveProperty('disabled', true));
    expect(btn.textContent).toContain('Start the gateway');
  });
});
