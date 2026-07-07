/**
 * Events shim (`src/lib/events`). `emit` must delegate to the real Tauri
 * event bus in the desktop shell — E2E helpers inject events like
 * 'server-install-request' through `window.__TAURI_TEST_API__.emit`, so a
 * no-op silently breaks deeplink-install / meta-tools E2E — and in the
 * browser web admin it must dispatch to listeners registered via the shim.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { emit as tauriEmit } from '@tauri-apps/api/event';
import { emit } from '@/lib/events';

const tauriEmitMock = vi.mocked(tauriEmit);

/** Minimal EventSource stand-in so the web-mode transport can subscribe. */
class StubEventSource {
  addEventListener(): void {}
  removeEventListener(): void {}
  close(): void {}
}

describe('events shim emit (Tauri env)', () => {
  it('delegates to @tauri-apps/api/event emit with the payload', async () => {
    await emit('server-install-request', { serverId: 'srv-1' });
    expect(tauriEmitMock).toHaveBeenCalledWith('server-install-request', { serverId: 'srv-1' });
  });

  it('delegates with undefined payload', async () => {
    await emit('ping');
    expect(tauriEmitMock).toHaveBeenCalledWith('ping', undefined);
  });
});

describe('events shim emit (web mode)', () => {
  afterEach(() => {
    // Restore the Tauri flag (setup.ts default) and drop the fresh module
    // graph so other suites keep exercising the Tauri path.
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {},
      writable: true,
      configurable: true,
    });
    vi.resetModules();
    vi.unstubAllGlobals();
  });

  it('dispatches to locally registered listeners, not Tauri IPC', async () => {
    // Web mode: no Tauri flag; fresh module graph so the facade picks HTTP.
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.resetModules();
    vi.stubGlobal('EventSource', StubEventSource);

    const events = await import('@/lib/events');
    const handler = vi.fn();
    await events.listen('server-status-changed', handler);
    await events.emit('server-status-changed', { serverId: 's1', status: 'running' });

    expect(handler).toHaveBeenCalledWith({
      event: 'server-status-changed',
      payload: { serverId: 's1', status: 'running' },
    });
    expect(tauriEmitMock).not.toHaveBeenCalled();
  });

  it('unsubscribed listeners no longer receive emitted events', async () => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.resetModules();
    vi.stubGlobal('EventSource', StubEventSource);

    const events = await import('@/lib/events');
    const handler = vi.fn();
    const unlisten = await events.listen('client-changed', handler);
    unlisten();
    await events.emit('client-changed', { action: 'registered' });

    expect(handler).not.toHaveBeenCalled();
  });
});
