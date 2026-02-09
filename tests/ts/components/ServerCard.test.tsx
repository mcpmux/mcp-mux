import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ServerCard } from '../../../apps/desktop/src/features/registry/ServerCard';
import type { ServerViewModel } from '../../../apps/desktop/src/types/registry';

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

describe('ServerCard', () => {
  const defaultProps = {
    onInstall: vi.fn(),
    onUninstall: vi.fn(),
    onViewDetails: vi.fn(),
  };

  describe('icon rendering', () => {
    it('should render fallback icon when icon is null', () => {
      const server = makeServer({ icon: null });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByTestId('server-icon-fallback')).toHaveTextContent('📦');
    });

    it('should render emoji icon as text', () => {
      const server = makeServer({ icon: '🔐' });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByTestId('server-icon-emoji')).toHaveTextContent('🔐');
    });

    it('should render URL icon as img element', () => {
      const server = makeServer({
        icon: 'https://avatars.githubusercontent.com/u/314135?v=4',
      });
      render(<ServerCard server={server} {...defaultProps} />);
      const img = screen.getByTestId('server-icon-img');
      expect(img.tagName).toBe('IMG');
      expect(img).toHaveAttribute(
        'src',
        'https://avatars.githubusercontent.com/u/314135?v=4'
      );
    });

    it('should show fallback when image fails to load', () => {
      const server = makeServer({
        icon: 'https://example.com/broken-icon.png',
      });
      render(<ServerCard server={server} {...defaultProps} />);
      const img = screen.getByTestId('server-icon-img');
      fireEvent.error(img);
      expect(screen.getByTestId('server-icon-fallback')).toHaveTextContent('📦');
    });
  });

  describe('server info', () => {
    it('should render server name', () => {
      const server = makeServer({ name: 'GitHub MCP Server' });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('GitHub MCP Server')).toBeInTheDocument();
    });

    it('should render description', () => {
      const server = makeServer({ description: 'Manage GitHub repos' });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('Manage GitHub repos')).toBeInTheDocument();
    });

    it('should render categories', () => {
      const server = makeServer({
        categories: ['cloud', 'developer-tools', 'productivity'],
      });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('cloud')).toBeInTheDocument();
      expect(screen.getByText('developer-tools')).toBeInTheDocument();
      expect(screen.getByText('productivity')).toBeInTheDocument();
    });

    it('should truncate categories beyond 3', () => {
      const server = makeServer({
        categories: ['cloud', 'developer-tools', 'productivity', 'extra'],
      });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('+1')).toBeInTheDocument();
    });
  });

  describe('actions', () => {
    it('should render Install button for non-installed server', () => {
      const server = makeServer({ is_installed: false });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('Install')).toBeInTheDocument();
    });

    it('should render Uninstall button for installed server', () => {
      const server = makeServer({ is_installed: true });
      render(<ServerCard server={server} {...defaultProps} />);
      expect(screen.getByText('Uninstall')).toBeInTheDocument();
    });

    it('should call onViewDetails when card is clicked', () => {
      const server = makeServer();
      render(<ServerCard server={server} {...defaultProps} />);
      fireEvent.click(screen.getByTestId(`server-card-${server.id}`));
      expect(defaultProps.onViewDetails).toHaveBeenCalledWith(server);
    });
  });
});
