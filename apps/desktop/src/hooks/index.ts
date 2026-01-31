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

