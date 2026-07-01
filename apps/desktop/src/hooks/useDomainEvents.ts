/**
 * Domain events hook — re-exports the transport-aware implementation (Tauri IPC + admin SSE).
 *
 * @deprecated Import from `@/lib/backend/events` directly in new code.
 */
export {
  useDomainEvents,
  useSpaceEvents,
  useServerStatusEvents,
  useServerAuthProgress,
  useClientEvents,
  useGatewayEvents,
  default,
} from '@/lib/backend/events/useDomainEvents';

export type {
  DomainEventChannel,
  DomainEventPayload,
  SpaceChangedPayload,
  ServerChangedPayload,
  ServerUpdateAvailablePayload,
  ServerStatusChangedPayload,
  ServerAuthProgressPayload,
  ServerFeaturesRefreshedPayload,
  FeatureSetChangedPayload,
  ClientChangedPayload,
  ClientGrantChangedPayload,
  GatewayChangedPayload,
  MCPNotificationPayload,
  PayloadTypeMap,
  ChannelCallback,
  AllEventsCallback,
} from '@/lib/backend/events/useDomainEvents';

/** @deprecated Use `ClientGrantChangedPayload`. */
export type { ClientGrantChangedPayload as GrantsChangedPayload } from '@/lib/backend/events/useDomainEvents';
