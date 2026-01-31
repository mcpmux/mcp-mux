/**
 * Server Manager API - Event-driven server connection management
 *
 * This replaces the old polling-based approach with Tauri events:
 * - Backend emits events: server-status-changed, server-auth-progress, server-features-refreshed
 * - UI listens to events and updates state accordingly
 * - No more polling for status changes
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

/**
 * Connection status - matches backend ConnectionStatus enum
 */
export type ConnectionStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "refreshing"
  | "oauth_required"  // Backend sends "oauth_required" for OAuth servers needing auth
  | "authenticating"
  | "error";

/**
 * Server status response from get_server_statuses
 */
export interface ServerStatusResponse {
  server_id: string;
  status: ConnectionStatus;
  flow_id: number;
  has_connected_before: boolean;
  message: string | null;
}

// Re-use ServerFeature from serverFeatures.ts to avoid duplication
import type { ServerFeature } from "./serverFeatures";
import { listServerFeaturesByServer } from "./serverFeatures";

/**
 * Cached features from a server
 */
export interface CachedFeatures {
  tools: ServerFeature[];
  prompts: ServerFeature[];
  resources: ServerFeature[];
}

/**
 * Server status changed event payload
 */
export interface ServerStatusEvent {
  type: "status_changed";
  server_id: string;
  space_id: string;
  status: ConnectionStatus;
  flow_id: number;
  has_connected_before: boolean;
  message?: string;
  features?: CachedFeatures;
}

/**
 * Auth progress event payload (during OAuth flow)
 */
export interface AuthProgressEvent {
  type: "auth_progress";
  server_id: string;
  space_id: string;
  remaining_seconds: number;
  flow_id: number;
}

/**
 * Features updated event payload
 */
export interface FeaturesUpdatedEvent {
  type: "features_updated";
  server_id: string;
  space_id: string;
  features: CachedFeatures;
  added: string[];
  removed: string[];
}

/**
 * Union type for all server events
 */
export type ServerEvent =
  | ServerStatusEvent
  | AuthProgressEvent
  | FeaturesUpdatedEvent;

// ============================================================================
// Commands (UI → Backend)
// ============================================================================

/**
 * Get all server statuses for a space
 */
export async function getServerStatuses(
  spaceId: string
): Promise<Record<string, ServerStatusResponse>> {
  return invoke<Record<string, ServerStatusResponse>>("get_server_statuses", {
    spaceId,
  });
}

/**
 * Enable a server and attempt connection
 *
 * The backend will:
 * 1. Update database (enabled = true)
 * 2. Set status = Connecting, emit event
 * 3. Attempt connection
 * 4. Emit Connected/AuthRequired/Error event
 */
export async function enableServer(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("enable_server_v2", { spaceId, serverId });
}

/**
 * Disable a server (cancels any active operations)
 */
export async function disableServer(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("disable_server_v2", { spaceId, serverId });
}

/**
 * Start OAuth flow (from AuthRequired state)
 *
 * Handles debounce: if called within 2s of last browser open, ignores silently.
 * If >= 2s, reopens the browser with the existing auth URL.
 */
export async function startAuth(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("start_auth_v2", { spaceId, serverId });
}

/**
 * Cancel OAuth flow
 */
export async function cancelAuth(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("cancel_auth_v2", { spaceId, serverId });
}

/**
 * Retry connection (from Error state)
 */
export async function retryConnection(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("retry_connection", { spaceId, serverId });
}

/**
 * Logout server - Clear OAuth tokens but keep enabled
 * 
 * Preserves: DCR registration, input values, enabled flag
 * Clears: OAuth tokens, oauth_connected flag
 * Result: State = auth_required, user must re-authenticate
 */
export async function logoutServer(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("logout_server", { spaceId, serverId });
}

/**
 * Disconnect server - Stop connection but keep enabled and preserve credentials
 * 
 * Preserves: Everything (tokens, DCR, inputs, enabled flag)
 * Result: State = auth_required (OAuth) or disconnected (non-OAuth)
 * Use case: Temporary pause, quick reconnect possible
 */
export async function disconnectServerV2(
  spaceId: string,
  serverId: string
): Promise<void> {
  return invoke("disconnect_server_v2", { spaceId, serverId });
}

// ============================================================================
// Event Listeners (Backend → UI)
// ============================================================================

/**
 * Listen for server status changes
 *
 * @param callback Called when a server's status changes
 * @returns Unlisten function to stop listening
 */
export async function onServerStatus(
  callback: (event: ServerStatusEvent) => void
): Promise<UnlistenFn> {
  return listen<{
    space_id: string;
    server_id: string;
    status: ConnectionStatus;
    has_connected_before: boolean;
    flow_id: number;
    message?: string;
  }>("server-status-changed", (event) => {
    callback({
      type: "status_changed",
      ...event.payload,
    });
  });
}

/**
 * Listen for auth progress updates (during OAuth flow)
 *
 * @param callback Called with remaining seconds
 * @returns Unlisten function to stop listening
 */
export async function onAuthProgress(
  callback: (event: AuthProgressEvent) => void
): Promise<UnlistenFn> {
  return listen<{
    space_id: string;
    server_id: string;
    remaining_seconds: number;
    flow_id: number;
  }>("server-auth-progress", (event) => {
    callback({
      type: "auth_progress",
      ...event.payload,
    });
  });
}

/**
 * Listen for feature updates
 *
 * @param callback Called when server features change
 * @returns Unlisten function to stop listening
 */
export async function onFeaturesUpdated(
  callback: (event: FeaturesUpdatedEvent) => void
): Promise<UnlistenFn> {
  return listen<{
    space_id: string;
    server_id: string;
    added: string[];
    removed: string[];
  }>("server-features-refreshed", async (event) => {
    const { space_id, server_id, added, removed } = event.payload;
    const allFeatures = await listServerFeaturesByServer(space_id, server_id);
    const features: CachedFeatures = {
      tools: allFeatures.filter((f) => f.feature_type === "tool"),
      prompts: allFeatures.filter((f) => f.feature_type === "prompt"),
      resources: allFeatures.filter((f) => f.feature_type === "resource"),
    };
    callback({
      type: "features_updated",
      space_id,
      server_id,
      features,
      added,
      removed,
    });
  });
}

// ============================================================================
// Helper: Get button label based on state
// ============================================================================

/**
 * Get the appropriate button label based on connection status and history
 *
 * @param status Current connection status
 * @param hasConnectedBefore Whether user has successfully connected before
 * @returns Button label string
 */
export function getConnectButtonLabel(
  status: ConnectionStatus,
  hasConnectedBefore: boolean
): string {
  if (status === "oauth_required" || status === "error") {
    return hasConnectedBefore ? "Reconnect" : "Connect";
  }
  if (status === "authenticating") {
    return "Authenticating...";
  }
  if (status === "connecting") {
    return "Connecting...";
  }
  return "Connect";
}

/**
 * Get the appropriate action for current state
 *
 * @param status Current connection status
 * @returns Action type
 */
export function getServerAction(
  status: ConnectionStatus
):
  | "enable"
  | "disable"
  | "connect"
  | "cancel"
  | "retry"
  | "connected"
  | "connecting" {
  switch (status) {
    case "disconnected":
      return "enable";
    case "connecting":
    case "refreshing":
      return "connecting";
    case "connected":
      return "connected";
    case "oauth_required":
      return "connect";
    case "authenticating":
      return "cancel";
    case "error":
      return "retry";
    default:
      return "enable";
  }
}
