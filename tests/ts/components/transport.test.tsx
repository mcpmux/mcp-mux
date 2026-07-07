/**
 * Transport facade (`src/lib/transport`). In the test env `setup.ts` marks the
 * environment as Tauri (`__TAURI_INTERNALS__`), so the facade selects the Tauri
 * transport — which must forward to `invoke`/`listen` with the SAME arity and
 * argument shapes the call sites used (the contract the desktop relies on), and
 * expose the desktop capability set.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { call, subscribe, capabilities, transport } from '@/lib/transport';
import { createHttpTransport } from '@/lib/transport/http';

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

describe('transport facade (Tauri env)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('call(cmd) forwards to invoke(cmd) with no args', async () => {
    invokeMock.mockResolvedValueOnce('ok');
    await call('list_spaces');
    expect(invokeMock).toHaveBeenCalledWith('list_spaces');
  });

  it('call(cmd, args) forwards to invoke(cmd, args)', async () => {
    invokeMock.mockResolvedValueOnce({ id: '1' });
    await call('get_space', { id: '1' });
    expect(invokeMock).toHaveBeenCalledWith('get_space', { id: '1' });
  });

  it('call returns invoke result', async () => {
    invokeMock.mockResolvedValueOnce([{ id: 'a' }]);
    const r = await call<Array<{ id: string }>>('list_spaces');
    expect(r).toEqual([{ id: 'a' }]);
  });

  it('subscribe maps to listen and returns an unsubscribe fn', async () => {
    const unlisten = vi.fn();
    listenMock.mockResolvedValueOnce(unlisten);
    const handler = vi.fn();
    const unsub = subscribe('client-changed', handler);

    expect(listenMock).toHaveBeenCalledWith('client-changed', expect.any(Function));

    // The listen callback delivers `event.payload` to the handler as payload.
    const cb = listenMock.mock.calls[0][1] as (e: { payload: unknown }) => void;
    cb({ payload: { action: 'registered' } });
    expect(handler).toHaveBeenCalledWith({ action: 'registered' });

    // Unsubscribe resolves the underlying unlisten.
    unsub();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalled();
  });

  it('exposes the desktop capability set', () => {
    expect(capabilities).toMatchObject({
      dialog: true,
      fsWrite: true,
      tray: true,
      deepLink: true,
      autostart: true,
      updater: true,
    });
    expect(transport.capabilities).toBe(capabilities);
  });
});

/**
 * The HTTP transport must multiplex ALL subscriptions over ONE EventSource:
 * browsers cap concurrent connections per origin (~6 on HTTP/1.1), and the
 * web admin registers 11+ listeners at boot — one SSE stream per subscribe()
 * starves every other fetch and freezes the UI.
 */
class MockEventSource {
  static instances: MockEventSource[] = [];
  url: string;
  closed = false;
  private listeners = new Map<string, Set<(e: MessageEvent) => void>>();

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  addEventListener(name: string, listener: (e: MessageEvent) => void): void {
    let set = this.listeners.get(name);
    if (!set) {
      set = new Set();
      this.listeners.set(name, set);
    }
    set.add(listener);
  }

  removeEventListener(name: string, listener: (e: MessageEvent) => void): void {
    this.listeners.get(name)?.delete(listener);
  }

  close(): void {
    this.closed = true;
  }

  /** Simulate the server sending a named SSE event. */
  dispatch(name: string, data: string): void {
    for (const listener of [...(this.listeners.get(name) ?? [])]) {
      listener(new MessageEvent(name, { data }));
    }
  }
}

describe('http transport SSE multiplexing', () => {
  beforeEach(() => {
    MockEventSource.instances = [];
    vi.stubGlobal('EventSource', MockEventSource);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('multiple subscribe() calls share exactly one EventSource', () => {
    const http = createHttpTransport({ getToken: () => 'tok' });
    http.subscribe('client-changed', vi.fn());
    http.subscribe('server-status-changed', vi.fn());
    http.subscribe('client-changed', vi.fn());
    http.subscribe('space-changed', vi.fn());

    expect(MockEventSource.instances).toHaveLength(1);
    expect(MockEventSource.instances[0].url).toBe('/admin/api/events?token=tok');
  });

  it('two handlers on the same event name both fire', () => {
    const http = createHttpTransport({ getToken: () => 'tok' });
    const h1 = vi.fn();
    const h2 = vi.fn();
    http.subscribe('client-changed', h1);
    http.subscribe('client-changed', h2);

    MockEventSource.instances[0].dispatch('client-changed', JSON.stringify({ action: 'x' }));

    expect(h1).toHaveBeenCalledWith({ action: 'x' });
    expect(h2).toHaveBeenCalledWith({ action: 'x' });
  });

  it('unsubscribing one handler keeps the other working', () => {
    const http = createHttpTransport({ getToken: () => 'tok' });
    const h1 = vi.fn();
    const h2 = vi.fn();
    const unsub1 = http.subscribe('client-changed', h1);
    http.subscribe('client-changed', h2);

    unsub1();
    MockEventSource.instances[0].dispatch('client-changed', JSON.stringify({ n: 1 }));

    expect(h1).not.toHaveBeenCalled();
    expect(h2).toHaveBeenCalledWith({ n: 1 });
    expect(MockEventSource.instances[0].closed).toBe(false);
  });

  it('last unsubscribe closes the EventSource; next subscribe reopens', () => {
    const http = createHttpTransport({ getToken: () => 'tok' });
    const unsubA = http.subscribe('client-changed', vi.fn());
    const unsubB = http.subscribe('server-status-changed', vi.fn());

    unsubA();
    expect(MockEventSource.instances[0].closed).toBe(false);
    unsubB();
    expect(MockEventSource.instances[0].closed).toBe(true);

    http.subscribe('client-changed', vi.fn());
    expect(MockEventSource.instances).toHaveLength(2);
    expect(MockEventSource.instances[1].closed).toBe(false);
  });
});
