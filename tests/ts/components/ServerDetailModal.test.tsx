import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { ServerDetailModal } from '../../../apps/desktop/src/features/registry/ServerDetailModal';
import type { ServerViewModel } from '../../../apps/desktop/src/types/registry';
import { renderWithI18n } from '../render-with-i18n.helpers';

function makeServer(overrides: Partial<ServerViewModel> = {}): ServerViewModel {
  return {
    id: 'com.test-server',
    name: 'Test Server',
    description: 'A test MCP server',
    alias: 'test',
    icon: null,
    auth: { type: 'none' },
    transport: {
      type: 'http',
      url: 'https://example.com/mcp',
      headers: {},
      metadata: { inputs: [] },
    },
    categories: ['developer-tools'],
    publisher: null,
    source: { type: 'Registry', url: 'https://registry.mcpmux.com', name: 'McpMux Registry' },
    is_installed: false,
    enabled: false,
    oauth_connected: false,
    input_values: {},
    connection_status: 'disconnected',
    missing_required_inputs: false,
    last_error: null,
    ...overrides,
  };
}

describe('ServerDetailModal', () => {
  const defaultProps = {
    onClose: vi.fn(),
    onInstall: vi.fn(),
    onUninstall: vi.fn(),
  };

  it('should render server name', () => {
    const server = makeServer({ name: 'Cloudflare Workers' });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('Cloudflare Workers')).toBeInTheDocument();
  });

  it('should render fallback icon when icon is null', () => {
    const server = makeServer({ icon: null });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByTestId('server-icon-fallback')).toHaveTextContent('📦');
  });

  it('should render emoji icon as text', () => {
    const server = makeServer({ icon: '🔐' });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByTestId('server-icon-emoji')).toHaveTextContent('🔐');
  });

  it('should render URL icon as img element', () => {
    const server = makeServer({
      icon: 'https://avatars.githubusercontent.com/u/314135?v=4',
    });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    const img = screen.getByTestId('server-icon-img');
    expect(img.tagName).toBe('IMG');
    expect(img).toHaveAttribute(
      'src',
      'https://avatars.githubusercontent.com/u/314135?v=4'
    );
  });

  it('should render description', () => {
    const server = makeServer({ description: 'Manages KV and R2 buckets' });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('Manages KV and R2 buckets')).toBeInTheDocument();
  });

  it('should render categories', () => {
    const server = makeServer({ categories: ['cloud', 'developer-tools'] });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('cloud')).toBeInTheDocument();
    expect(screen.getByText('developer-tools')).toBeInTheDocument();
  });

  it('should render hosting type for remote server', () => {
    const server = makeServer({
      transport: {
        type: 'http',
        url: 'https://example.com/mcp',
        headers: {},
        metadata: { inputs: [] },
      },
    });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText(/Remote Server/)).toBeInTheDocument();
  });

  it('should render Install button for non-installed server', () => {
    const server = makeServer({ is_installed: false });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('Install')).toBeInTheDocument();
  });

  it('should render Uninstall button for installed server', () => {
    const server = makeServer({ is_installed: true });
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('Uninstall')).toBeInTheDocument();
  });

  it('should render View JSON button in footer', () => {
    const server = makeServer();
    renderWithI18n(<ServerDetailModal server={server} {...defaultProps} />);
    expect(screen.getByText('View JSON')).toBeInTheDocument();
  });
});
