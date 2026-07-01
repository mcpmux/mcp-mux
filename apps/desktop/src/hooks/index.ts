/**
 * Hooks - React hooks for the McpMux desktop application
 */

// Data synchronization
export { useDataSync } from './useDataSync';

// Domain events (event-driven architecture)
export {
  useDomainEvents,
  useSpaceEvents,
  useServerStatusEvents,
  useServerAuthProgress,
  useClientEvents,
  useGatewayEvents,
} from './useDomainEvents';

export type {
  DomainEventChannel,
  DomainEventPayload,
  SpaceChangedPayload,
  ServerChangedPayload,
  ServerStatusChangedPayload,
  ServerAuthProgressPayload,
  ServerFeaturesRefreshedPayload,
  FeatureSetChangedPayload,
  ClientChangedPayload,
  GrantsChangedPayload,
  GatewayChangedPayload,
  MCPNotificationPayload,
} from './useDomainEvents';

// Server management
export { useServerManager } from './useServerManager';

// Space management
export { useSpaces } from './useSpaces';

// Event hooks (re-exported shims from @/lib/backend/events)
export { useMetaToolEvents, useMetaToolEventListener } from './useMetaToolEvents';
export { useOAuthClientEvents, useOAuthClientEventListener } from './useOAuthClientEvents';
export type { OAuthClientChangedPayload } from './useOAuthClientEvents';
export { useWorkspaceEvents, useWorkspaceEventListener } from './useWorkspaceEvents';
export type {
  WorkspaceEventChannel,
  WorkspaceBindingChangedPayload,
  WorkspaceNeedsBindingPayload,
} from './useWorkspaceEvents';

