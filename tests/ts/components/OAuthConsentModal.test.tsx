/**
 * The consent dialog must show where approving sends the user and which
 * client is asking, and declining must never hand an app callback to the OS.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act, fireEvent, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { OAuthConsentModal } from '@/components/OAuthConsentModal';
import { openRedirectUrl } from '@/lib/oauthRedirect';

const { handlers } = vi.hoisted(() => ({
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));
vi.mock('@/lib/oauthRedirect', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/oauthRedirect')>()),
  openRedirectUrl: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const details = {
  requestId: 'req-1',
  clientId: 'mcp_1234abcd',
  clientName: 'Cursor',
  redirectUri: 'cursor://anysphere.cursor-mcp/oauth/callback',
  scope: '',
  state: 'st',
  expiresAt: Math.floor(Date.now() / 1000) + 300,
  consentToken: 'token',
  firstTime: true,
};

async function openConsent(approveResponse: unknown, overrides: Partial<typeof details> = {}) {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'get_pending_consent') return { ...details, ...overrides };
    if (cmd === 'approve_oauth_consent') return approveResponse;
    return undefined;
  });
  render(<OAuthConsentModal />);
  await waitFor(() => expect(handlers.has('oauth-consent-request')).toBe(true));
  await act(async () => {
    handlers.get('oauth-consent-request')!({ payload: { requestId: 'req-1' } });
  });
  await screen.findByText('Allow Cursor to connect?');
}

beforeEach(() => {
  handlers.clear();
  invokeMock.mockReset();
  vi.mocked(openRedirectUrl).mockReset();
});

describe('OAuthConsentModal', () => {
  it('shows the client id, where approving returns, and first-time requests', async () => {
    await openConsent({ success: true, redirect_url: '', error: null });

    const box = screen.getByTestId('consent-client-details');
    expect(box).toHaveTextContent('cursor://anysphere.cursor-mcp');
    expect(box).toHaveTextContent('mcp_1234abcd');
    expect(box).toHaveTextContent('First time this app asks to connect');
  });

  it("shows the initial, not a known app's logo, on a first request", async () => {
    await openConsent({ success: true, redirect_url: '', error: null });
    expect(screen.queryByRole('img', { name: 'Cursor' })).toBeNull();
    expect(screen.getByText('C')).toBeInTheDocument();
  });

  it('shows the logo of a known app approved before', async () => {
    await openConsent({ success: true, redirect_url: '', error: null }, { firstTime: false });
    expect(screen.getByRole('img', { name: 'Cursor' })).toBeInTheDocument();
  });

  it('closes on Deny without launching an app callback', async () => {
    await openConsent({ success: true, redirect_url: '', error: null });

    fireEvent.click(screen.getByRole('button', { name: /deny/i }));

    await waitFor(() =>
      expect(screen.queryByText('Allow Cursor to connect?')).not.toBeInTheDocument()
    );
    expect(openRedirectUrl).not.toHaveBeenCalled();
  });

  it('sends a denial back to an http callback', async () => {
    const denial = 'http://127.0.0.1:8765/callback?error=access_denied&state=st';
    await openConsent({ success: true, redirect_url: denial, error: null });

    fireEvent.click(screen.getByRole('button', { name: /deny/i }));

    await waitFor(() => expect(openRedirectUrl).toHaveBeenCalledWith(denial));
  });
});
