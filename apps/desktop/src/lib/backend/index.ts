/**
 * Unified backend facade — three channels: data (commands), events (Phase 2), shell (desktop-only).
 * @see AGENTS.md Frontend Notes (`@/lib/backend` facade)
 */

export * from '../api';
export * from './data/transport';
export * from './data/fetch-api';
export * from './events';
export * as shell from './shell';
