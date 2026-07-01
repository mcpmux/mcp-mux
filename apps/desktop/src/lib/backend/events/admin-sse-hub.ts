/**
 * Single shared EventSource for web admin — avoids HTTP/1.1 connection starvation
 * when multiple hooks call `useDomainEvents()` (each used to open its own SSE).
 */

import { isTauri } from '../data/transport';

import type {
  AllEventsCallback,
  DomainEventChannel,
  DomainEventPayload,
} from './useDomainEvents';

/** All domain channels streamed over SSE. */
export const ADMIN_SSE_CHANNELS: DomainEventChannel[] = [
  'space-changed',
  'server-changed',
  'server-update-available',
  'server-status-changed',
  'server-auth-progress',
  'server-features-refreshed',
  'feature-set-changed',
  'client-changed',
  'client-grant-changed',
  'gateway-changed',
  'mcp-notification',
];

type ChannelHandler = (payload: DomainEventPayload) => void;
type RawChannelHandler = (payload: unknown) => void;

let sharedSource: EventSource | null = null;
let consumerCount = 0;
let sseEnabled = false;
const channelHandlers = new Map<DomainEventChannel, Set<ChannelHandler>>();
const allHandlers = new Set<AllEventsCallback>();
const lastEventListeners = new Set<(event: { channel: DomainEventChannel; payload: DomainEventPayload }) => void>();
/** Handlers for non-domain channels (workspace, meta-tool, etc.) sharing the same SSE connection. */
const rawChannelHandlers = new Map<string, Set<RawChannelHandler>>();
/** Raw channels that already have a single dispatcher on `sharedSource`. */
const rawChannelsAttached = new Set<string>();

/**
 * Dispatch an SSE frame to all registered handlers.
 */
function dispatch(channel: DomainEventChannel, payload: DomainEventPayload): void {
  const event = { channel, payload };
  lastEventListeners.forEach((listener) => listener(event));
  channelHandlers.get(channel)?.forEach((handler) => handler(payload));
  allHandlers.forEach((handler) => handler(channel, payload));
}

/**
 * Open the shared admin SSE connection when the first consumer attaches.
 */
function ensureSharedSource(): void {
  if (sharedSource || isTauri()) {
    return;
  }

  const source = new EventSource('/api/v1/events');
  sharedSource = source;

  for (const channel of ADMIN_SSE_CHANNELS) {
    source.addEventListener(channel, (event: MessageEvent<string>) => {
      try {
        const payload = JSON.parse(event.data) as DomainEventPayload;
        dispatch(channel, payload);
      } catch {
        // ignore malformed frames
      }
    });
  }

  for (const channel of rawChannelHandlers.keys()) {
    attachRawChannelListener(channel);
  }
}

/**
 * Attach one SSE listener per raw channel; dispatches to all handlers in the set.
 */
function attachRawChannelListener(channel: string): void {
  if (!sharedSource || rawChannelsAttached.has(channel)) {
    return;
  }
  rawChannelsAttached.add(channel);
  sharedSource.addEventListener(channel, (event: MessageEvent<string>) => {
    try {
      const payload = JSON.parse(event.data) as unknown;
      rawChannelHandlers.get(channel)?.forEach((handler) => handler(payload));
    } catch {
      // ignore malformed frames
    }
  });
}

/**
 * Close the shared SSE connection when the last consumer detaches.
 */
function releaseSharedSource(): void {
  if (consumerCount > 0 || !sharedSource) {
    return;
  }
  sharedSource.close();
  sharedSource = null;
  rawChannelsAttached.clear();
}

/**
 * Open the shared SSE connection once web admin startup sync begins.
 */
export function enableAdminSse(): void {
  if (isTauri()) {
    return;
  }
  sseEnabled = true;
  if (consumerCount > 0) {
    ensureSharedSource();
  }
}

/**
 * Register a hook instance as an SSE consumer (ref-counted).
 */
export function acquireAdminSseConsumer(): void {
  if (isTauri()) {
    return;
  }
  consumerCount += 1;
  if (sseEnabled) {
    ensureSharedSource();
  }
}

/**
 * Unregister an SSE consumer; closes the connection when ref-count hits zero.
 */
export function releaseAdminSseConsumer(): void {
  if (isTauri()) {
    return;
  }
  consumerCount = Math.max(0, consumerCount - 1);
  releaseSharedSource();
}

/**
 * Subscribe to one SSE channel on the shared connection.
 */
export function subscribeAdminSseChannel(
  channel: DomainEventChannel,
  handler: ChannelHandler
): () => void {
  if (!channelHandlers.has(channel)) {
    channelHandlers.set(channel, new Set());
  }
  channelHandlers.get(channel)!.add(handler);
  return () => {
    channelHandlers.get(channel)?.delete(handler);
  };
}

/**
 * Subscribe to all SSE channels on the shared connection.
 */
export function subscribeAdminSseAll(handler: AllEventsCallback): () => void {
  allHandlers.add(handler);
  return () => {
    allHandlers.delete(handler);
  };
}

/**
 * Listen for the most recent SSE event (for hook `lastEvent` state).
 */
export function onAdminSseLastEvent(
  listener: (event: { channel: DomainEventChannel; payload: DomainEventPayload }) => void
): () => void {
  lastEventListeners.add(listener);
  return () => {
    lastEventListeners.delete(listener);
  };
}

/**
 * Subscribe to a non-domain SSE channel on the shared connection (e.g. workspace,
 * meta-tool, OAuth channels). Attaches to the live source immediately if already
 * open; otherwise the listener is registered for when the source next opens.
 */
export function subscribeAdminSseRaw(channel: string, handler: RawChannelHandler): () => void {
  if (!rawChannelHandlers.has(channel)) {
    rawChannelHandlers.set(channel, new Set());
  }
  rawChannelHandlers.get(channel)!.add(handler);
  attachRawChannelListener(channel);

  return () => {
    rawChannelHandlers.get(channel)?.delete(handler);
  };
}
