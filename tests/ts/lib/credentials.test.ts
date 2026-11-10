import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  findPriorCredential,
  fillEmptyCredentialValues,
  getPriorCredentialValues,
  type PriorCredentialSummary,
} from '../../../apps/desktop/src/lib/api/credentials';

const mockedInvoke = vi.mocked(invoke);

describe('credentials API (Issue #227)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('findPriorCredential', () => {
    it('returns PriorCredentialSummary when found', async () => {
      const mockSummary: PriorCredentialSummary = {
        source_space_id: 'space-work-123',
        source_space_name: 'Work',
        server_id: 'io.github.github/github-mcp-server',
        available_keys: ['GITHUB_PERSONAL_ACCESS_TOKEN'],
        updated_at: '2026-11-09T16:00:00Z',
      };

      mockedInvoke.mockResolvedValueOnce(mockSummary);

      const result = await findPriorCredential(
        'space-personal-456',
        'io.github.github/github-mcp-server'
      );

      expect(mockedInvoke).toHaveBeenCalledWith('find_prior_credential', {
        spaceId: 'space-personal-456',
        serverId: 'io.github.github/github-mcp-server',
        inputId: undefined,
      });
      expect(result).toEqual(mockSummary);
    });

    it('returns null when no prior credentials exist', async () => {
      mockedInvoke.mockResolvedValueOnce(null);

      const result = await findPriorCredential(
        'space-personal-456',
        'io.github.github/github-mcp-server'
      );

      expect(result).toBeNull();
    });
  });

  describe('getPriorCredentialValues', () => {
    it('fetches decrypted values for prior server', async () => {
      const mockValues = {
        GITHUB_PERSONAL_ACCESS_TOKEN: 'ghp_secret_token_123',
      };

      mockedInvoke.mockResolvedValueOnce(mockValues);

      const result = await getPriorCredentialValues(
        'space-work-123',
        'io.github.github/github-mcp-server'
      );

      expect(mockedInvoke).toHaveBeenCalledWith('get_prior_credential_values', {
        sourceSpaceId: 'space-work-123',
        serverId: 'io.github.github/github-mcp-server',
      });
      expect(result).toEqual(mockValues);
    });
  });

  describe('fillEmptyCredentialValues', () => {
    it('fills blank form fields without replacing values already entered by the user', () => {
      expect(
        fillEmptyCredentialValues(
          {
            GITHUB_PERSONAL_ACCESS_TOKEN: 'user-entered-token',
            GITHUB_API_URL: '',
          },
          {
            GITHUB_PERSONAL_ACCESS_TOKEN: 'prior-space-token',
            GITHUB_API_URL: 'https://api.github.com',
          }
        )
      ).toEqual({
        GITHUB_PERSONAL_ACCESS_TOKEN: 'user-entered-token',
        GITHUB_API_URL: 'https://api.github.com',
      });
    });

    it('treats whitespace-only fields as blank', () => {
      expect(fillEmptyCredentialValues({ TOKEN: '   ' }, { TOKEN: 'prior-space-token' })).toEqual({
        TOKEN: 'prior-space-token',
      });
    });
  });
});
