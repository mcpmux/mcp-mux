/** @deprecated Prefer `@/lib/backend/events` */
export { useWorkspaceEvents, useWorkspaceEventListener } from '@/lib/backend/events';

export type {
  WorkspaceEventChannel,
  WorkspaceBindingChangedPayload,
  WorkspaceNeedsBindingPayload,
} from '@/lib/backend/events';

export { default } from '@/lib/backend/events/useWorkspaceEvents';
