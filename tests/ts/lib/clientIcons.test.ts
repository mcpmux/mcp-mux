import { describe, it, expect } from 'vitest';
import { resolveKnownClientKey } from '../../../apps/desktop/src/lib/clientIcons';

describe('resolveKnownClientKey', () => {
  describe('exact matches', () => {
    it('should resolve "cursor" to cursor', () => {
      expect(resolveKnownClientKey('cursor')).toBe('cursor');
    });

    it('should resolve "Cursor" (case-insensitive) to cursor', () => {
      expect(resolveKnownClientKey('Cursor')).toBe('cursor');
    });

    it('should resolve "claude" to claude', () => {
      expect(resolveKnownClientKey('claude')).toBe('claude');
    });

    it('should resolve "Claude Desktop" to claude', () => {
      expect(resolveKnownClientKey('Claude Desktop')).toBe('claude');
    });

    it('should resolve "vs code" to vscode', () => {
      expect(resolveKnownClientKey('vs code')).toBe('vscode');
    });

    it('should resolve "vscode" to vscode', () => {
      expect(resolveKnownClientKey('vscode')).toBe('vscode');
    });

    it('should resolve "Visual Studio Code" to vscode', () => {
      expect(resolveKnownClientKey('Visual Studio Code')).toBe('vscode');
    });

    it('should resolve "windsurf" to windsurf', () => {
      expect(resolveKnownClientKey('windsurf')).toBe('windsurf');
    });

    it('should resolve "codeium" to windsurf', () => {
      expect(resolveKnownClientKey('codeium')).toBe('windsurf');
    });
  });

  describe('prefix matches with parenthesised suffix', () => {
    it('should resolve "Claude Code (mcpmux)" to claude', () => {
      expect(resolveKnownClientKey('Claude Code (mcpmux)')).toBe('claude');
    });

    it('should resolve "Claude Desktop (some-server)" to claude', () => {
      expect(resolveKnownClientKey('Claude Desktop (some-server)')).toBe('claude');
    });

    it('should resolve "Cursor (my-project)" to cursor', () => {
      expect(resolveKnownClientKey('Cursor (my-project)')).toBe('cursor');
    });

    it('should resolve "VS Code (workspace)" to vscode', () => {
      expect(resolveKnownClientKey('VS Code (workspace)')).toBe('vscode');
    });

    it('should resolve "Windsurf (test)" to windsurf', () => {
      expect(resolveKnownClientKey('Windsurf (test)')).toBe('windsurf');
    });
  });

  describe('prefix matches with space-separated suffix', () => {
    it('should resolve "Claude Code v2" to claude', () => {
      expect(resolveKnownClientKey('Claude Code v2')).toBe('claude');
    });

    it('should resolve "Cursor beta" to cursor', () => {
      expect(resolveKnownClientKey('Cursor beta')).toBe('cursor');
    });
  });

  describe('non-matching names', () => {
    it('should return null for unknown client names', () => {
      expect(resolveKnownClientKey('unknown-client')).toBeNull();
    });

    it('should return null for empty string', () => {
      expect(resolveKnownClientKey('')).toBeNull();
    });

    it('should not match partial names without word boundary', () => {
      // "claudeXYZ" should NOT match "claude" — no word boundary
      expect(resolveKnownClientKey('claudeXYZ')).toBeNull();
    });

    it('should not match "cursorify" (no word boundary)', () => {
      expect(resolveKnownClientKey('cursorify')).toBeNull();
    });
  });

  describe('whitespace handling', () => {
    it('should trim leading/trailing whitespace', () => {
      expect(resolveKnownClientKey('  Cursor  ')).toBe('cursor');
    });

    it('should handle whitespace with suffix', () => {
      expect(resolveKnownClientKey('  Claude Code (mcpmux)  ')).toBe('claude');
    });
  });
});
