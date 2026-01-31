import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  SourceBadge,
  getUninstallLabel,
  getUninstallConfirmMessage,
} from '../../../apps/desktop/src/components/SourceBadge';
import type { InstallationSource } from '../../../apps/desktop/src/types/registry';

describe('SourceBadge', () => {
  describe('rendering', () => {
    it('should render null when source is undefined', () => {
      const { container } = render(<SourceBadge source={undefined} />);
      expect(container.firstChild).toBeNull();
    });

    it('should render registry badge', () => {
      const source: InstallationSource = { type: 'registry' };
      render(<SourceBadge source={source} />);

      const badge = screen.getByText('Registry');
      expect(badge).toBeInTheDocument();
      expect(badge).toHaveAttribute('title', 'Installed from registry');
      expect(badge).toHaveClass('bg-blue-100');
    });

    it('should render config file badge with file path in title', () => {
      const source: InstallationSource = {
        type: 'user_config',
        file_path: '/path/to/config.json',
      };
      render(<SourceBadge source={source} />);

      const badge = screen.getByText('Config File');
      expect(badge).toBeInTheDocument();
      expect(badge).toHaveAttribute('title', 'From config: /path/to/config.json');
      expect(badge).toHaveClass('bg-green-100');
    });

    it('should render manual entry badge', () => {
      const source: InstallationSource = { type: 'manual_entry' };
      render(<SourceBadge source={source} />);

      const badge = screen.getByText('Manual');
      expect(badge).toBeInTheDocument();
      expect(badge).toHaveAttribute('title', 'Manually added');
      expect(badge).toHaveClass('bg-gray-100');
    });

    it('should apply custom className', () => {
      const source: InstallationSource = { type: 'registry' };
      render(<SourceBadge source={source} className="custom-class" />);

      const badge = screen.getByText('Registry');
      expect(badge).toHaveClass('custom-class');
    });
  });
});

describe('getUninstallLabel', () => {
  it('should return "Uninstall" for undefined source', () => {
    expect(getUninstallLabel(undefined)).toBe('Uninstall');
  });

  it('should return "Uninstall" for registry source', () => {
    const source: InstallationSource = { type: 'registry' };
    expect(getUninstallLabel(source)).toBe('Uninstall');
  });

  it('should return "Remove from Config" for user_config source', () => {
    const source: InstallationSource = {
      type: 'user_config',
      file_path: '/path/to/config.json',
    };
    expect(getUninstallLabel(source)).toBe('Remove from Config');
  });

  it('should return "Remove" for manual_entry source', () => {
    const source: InstallationSource = { type: 'manual_entry' };
    expect(getUninstallLabel(source)).toBe('Remove');
  });
});

describe('getUninstallConfirmMessage', () => {
  it('should return standard message for registry source', () => {
    const source: InstallationSource = { type: 'registry' };
    const message = getUninstallConfirmMessage('Test Server', source);

    expect(message).toContain('Test Server');
    expect(message).toContain('reinstall it from the registry');
  });

  it('should return config-specific message for user_config source', () => {
    const source: InstallationSource = {
      type: 'user_config',
      file_path: '/path/to/config.json',
    };
    const message = getUninstallConfirmMessage('Test Server', source);

    expect(message).toContain('Test Server');
    expect(message).toContain('remove');
    expect(message).toContain('config file');
  });

  it('should return standard message for undefined source', () => {
    const message = getUninstallConfirmMessage('Test Server', undefined);

    expect(message).toContain('Test Server');
    expect(message).toContain('reinstall it from the registry');
  });
});
