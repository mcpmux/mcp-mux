import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ServerIcon } from '../../../apps/desktop/src/components/ServerIcon';

describe('ServerIcon', () => {
  describe('fallback rendering', () => {
    it('should render default fallback when icon is null', () => {
      render(<ServerIcon icon={null} />);
      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toBeInTheDocument();
      expect(fallback).toHaveTextContent('📦');
    });

    it('should render default fallback when icon is undefined', () => {
      render(<ServerIcon icon={undefined} />);
      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toBeInTheDocument();
      expect(fallback).toHaveTextContent('📦');
    });

    it('should render default fallback when icon is empty string', () => {
      render(<ServerIcon icon="" />);
      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toBeInTheDocument();
      expect(fallback).toHaveTextContent('📦');
    });

    it('should render custom fallback when specified', () => {
      render(<ServerIcon icon={null} fallback="🔌" />);
      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toHaveTextContent('🔌');
    });
  });

  describe('emoji rendering', () => {
    it('should render emoji icon as text', () => {
      render(<ServerIcon icon="🔐" />);
      const emoji = screen.getByTestId('server-icon-emoji');
      expect(emoji).toBeInTheDocument();
      expect(emoji).toHaveTextContent('🔐');
    });

    it('should render non-URL text as emoji', () => {
      render(<ServerIcon icon="test-icon" />);
      const emoji = screen.getByTestId('server-icon-emoji');
      expect(emoji).toBeInTheDocument();
      expect(emoji).toHaveTextContent('test-icon');
    });
  });

  describe('URL icon rendering', () => {
    it('should render HTTP URL as img element', () => {
      render(
        <ServerIcon icon="http://example.com/icon.png" />
      );
      const img = screen.getByTestId('server-icon-img');
      expect(img).toBeInTheDocument();
      expect(img.tagName).toBe('IMG');
      expect(img).toHaveAttribute('src', 'http://example.com/icon.png');
    });

    it('should render HTTPS URL as img element', () => {
      render(
        <ServerIcon icon="https://avatars.githubusercontent.com/u/314135?v=4" />
      );
      const img = screen.getByTestId('server-icon-img');
      expect(img).toBeInTheDocument();
      expect(img.tagName).toBe('IMG');
      expect(img).toHaveAttribute(
        'src',
        'https://avatars.githubusercontent.com/u/314135?v=4'
      );
    });

    it('should apply custom className to img element', () => {
      render(
        <ServerIcon
          icon="https://example.com/icon.png"
          className="w-12 h-12 rounded-lg"
        />
      );
      const img = screen.getByTestId('server-icon-img');
      expect(img).toHaveClass('w-12', 'h-12', 'rounded-lg');
    });

    it('should apply default className to img element', () => {
      render(
        <ServerIcon icon="https://example.com/icon.png" />
      );
      const img = screen.getByTestId('server-icon-img');
      expect(img).toHaveClass('w-9', 'h-9', 'object-contain');
    });

    it('should show fallback when image fails to load', () => {
      render(
        <ServerIcon icon="https://example.com/broken-icon.png" />
      );
      const img = screen.getByTestId('server-icon-img');
      expect(img).toBeInTheDocument();

      // Simulate image load error
      fireEvent.error(img);

      // Should now show fallback
      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toBeInTheDocument();
      expect(fallback).toHaveTextContent('📦');
    });

    it('should show custom fallback when image fails to load', () => {
      render(
        <ServerIcon icon="https://example.com/broken.png" fallback="⚠️" />
      );
      const img = screen.getByTestId('server-icon-img');
      fireEvent.error(img);

      const fallback = screen.getByTestId('server-icon-fallback');
      expect(fallback).toHaveTextContent('⚠️');
    });
  });
});
