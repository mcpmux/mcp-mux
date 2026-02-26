import { describe, it, expect } from 'vitest';
import {
  getConnectButtonLabel,
  getServerAction,
  type ConnectionStatus,
} from '@/lib/api/serverManager';

describe('getConnectButtonLabel', () => {
  it('oauth_required with no prior connection returns "Connect"', () => {
    expect(getConnectButtonLabel('oauth_required', false)).toBe('Connect');
  });

  it('oauth_required with prior connection returns "Reconnect"', () => {
    expect(getConnectButtonLabel('oauth_required', true)).toBe('Reconnect');
  });

  it('error with no prior connection returns "Connect"', () => {
    expect(getConnectButtonLabel('error', false)).toBe('Connect');
  });

  it('error with prior connection returns "Reconnect"', () => {
    expect(getConnectButtonLabel('error', true)).toBe('Reconnect');
  });

  it('authenticating returns "Authenticating..."', () => {
    expect(getConnectButtonLabel('authenticating', false)).toBe('Authenticating...');
  });

  it('connecting returns "Connecting..."', () => {
    expect(getConnectButtonLabel('connecting', false)).toBe('Connecting...');
  });

  it('connected returns "Connect"', () => {
    expect(getConnectButtonLabel('connected', false)).toBe('Connect');
  });

  it('disconnected returns "Connect"', () => {
    expect(getConnectButtonLabel('disconnected', false)).toBe('Connect');
  });
});

describe('getServerAction', () => {
  it('disconnected returns "enable"', () => {
    expect(getServerAction('disconnected')).toBe('enable');
  });

  it('connecting returns "connecting"', () => {
    expect(getServerAction('connecting')).toBe('connecting');
  });

  it('refreshing returns "connecting"', () => {
    expect(getServerAction('refreshing')).toBe('connecting');
  });

  it('connected returns "connected"', () => {
    expect(getServerAction('connected')).toBe('connected');
  });

  it('oauth_required returns "connect"', () => {
    expect(getServerAction('oauth_required')).toBe('connect');
  });

  it('authenticating returns "cancel"', () => {
    expect(getServerAction('authenticating')).toBe('cancel');
  });

  it('error returns "retry"', () => {
    expect(getServerAction('error')).toBe('retry');
  });

  it('all statuses return defined values', () => {
    const statuses: ConnectionStatus[] = [
      'disconnected',
      'connecting',
      'connected',
      'refreshing',
      'oauth_required',
      'authenticating',
      'error',
    ];
    for (const status of statuses) {
      expect(getServerAction(status)).toBeDefined();
    }
  });
});
