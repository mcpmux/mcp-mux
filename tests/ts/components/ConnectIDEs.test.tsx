import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../../apps/desktop/src/lib/api/clientInstall', () => ({
  addToVscode: vi.fn(),
  addToCursor: vi.fn(),
}));

import { ConnectIDEs } from '../../../apps/desktop/src/components/ConnectIDEs';
import {
  addToVscode,
  addToCursor,
} from '../../../apps/desktop/src/lib/api/clientInstall';

const mockedAddVscode = vi.mocked(addToVscode);
const mockedAddCursor = vi.mocked(addToCursor);

describe('ConnectIDEs', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should render the card title', () => {
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );
    expect(screen.getByText('Connect Your IDEs')).toBeInTheDocument();
  });

  it('should render icon buttons for all entries', () => {
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );
    expect(screen.getByTestId('client-icon-vscode')).toBeInTheDocument();
    expect(screen.getByTestId('client-icon-cursor')).toBeInTheDocument();
    expect(screen.getByTestId('client-icon-claude-code')).toBeInTheDocument();
    expect(screen.getByTestId('client-icon-copy-config')).toBeInTheDocument();
  });

  it('should show labels under icons', () => {
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );
    expect(screen.getByText('VS Code')).toBeInTheDocument();
    expect(screen.getByText('Cursor')).toBeInTheDocument();
    expect(screen.getByText('Claude')).toBeInTheDocument();
    expect(screen.getByText('JSON')).toBeInTheDocument();
  });

  it('should show popover when clicking a client icon', async () => {
    const user = userEvent.setup();
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-vscode'));

    expect(screen.getByTestId('client-popover')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Add to VS Code/i })).toBeInTheDocument();
  });

  it('should close popover when clicking the same icon again', async () => {
    const user = userEvent.setup();
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-vscode'));
    expect(screen.getByTestId('client-popover')).toBeInTheDocument();

    await user.click(screen.getByTestId('client-icon-vscode'));
    expect(screen.queryByTestId('client-popover')).not.toBeInTheDocument();
  });

  it('should call addToVscode when clicking Add in VS Code popover', async () => {
    const user = userEvent.setup();
    mockedAddVscode.mockResolvedValue(undefined);

    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-vscode'));
    await user.click(screen.getByRole('button', { name: /Add to VS Code/i }));

    expect(mockedAddVscode).toHaveBeenCalledWith('http://localhost:45818');
  });

  it('should call addToCursor when clicking Add in Cursor popover', async () => {
    const user = userEvent.setup();
    mockedAddCursor.mockResolvedValue(undefined);

    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-cursor'));
    await user.click(screen.getByRole('button', { name: /Add to Cursor/i }));

    expect(mockedAddCursor).toHaveBeenCalledWith('http://localhost:45818');
  });

  it('should show Copy command for Claude Code', async () => {
    const user = userEvent.setup();
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-claude-code'));

    expect(screen.getByText('Claude Code')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Copy command/i })).toBeInTheDocument();
  });

  it('should copy CLI command for Claude Code', async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    });

    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-claude-code'));
    await user.click(screen.getByRole('button', { name: /Copy command/i }));

    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining('claude mcp add')
    );
  });

  it('should copy config when clicking Copy Config icon', async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    });

    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );

    await user.click(screen.getByTestId('client-icon-copy-config'));
    await user.click(screen.getByTestId('copy-config-btn'));

    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining('localhost:45818/mcp')
    );
  });

  it('should disable Add button when gateway not running', async () => {
    const user = userEvent.setup();
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={false} />
    );

    await user.click(screen.getByTestId('client-icon-vscode'));

    const addBtn = screen.getByRole('button', { name: /Add to VS Code/i });
    expect(addBtn).toBeDisabled();
  });

  it('should show orange indicator when gateway not running', () => {
    const { container } = render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={false} />
    );
    const dot = container.querySelector('.bg-orange-500');
    expect(dot).toBeInTheDocument();
  });

  it('should show gateway URL', () => {
    render(
      <ConnectIDEs gatewayUrl="http://localhost:45818" gatewayRunning={true} />
    );
    expect(screen.getByText('http://localhost:45818')).toBeInTheDocument();
  });
});
