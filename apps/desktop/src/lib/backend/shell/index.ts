import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { emit, listen, type Event, type UnlistenFn } from '@tauri-apps/api/event';
import { open, type OpenDialogOptions } from '@tauri-apps/plugin-dialog';
import { relaunch } from '@tauri-apps/plugin-process';
import type { Update } from '@tauri-apps/plugin-updater';

import type { ExportConfigRequest } from '@/lib/api/configExport';
import type { AdminWebSettings } from '@/lib/api/settings';

import { apiCall, isTauri } from '../data/transport';

export { isTauri };
export type { Event, UnlistenFn, Update };

declare global {
  interface Window {
    __TAURI_TEST_API__?: {
      invoke: typeof invoke;
      emit: typeof emit;
    };
  }
}

/** Window chrome control actions for the custom title bar. */
export type WindowControlAction = 'minimize' | 'maximize' | 'close';

/**
 * Expose Tauri invoke/emit on window for E2E tests (desktop shell only).
 */
export function initTauriTestApi(): void {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return;
  }
  window.__TAURI_TEST_API__ = { invoke, emit };
}

/**
 * Return true when the URL targets a loopback HTTP(S) OAuth callback.
 */
function isLocalhostHttpUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const isLocalhost = parsed.hostname === 'localhost' || parsed.hostname === '127.0.0.1';
    return isLocalhost && (parsed.protocol === 'http:' || parsed.protocol === 'https:');
  } catch {
    return false;
  }
}

/**
 * Open a URL using the system's default handler.
 *
 * In web admin mode the browser opens the URL directly. Desktop uses Tauri
 * so custom protocol handlers (e.g. `cursor://`) reach the OS.
 */
export async function openUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  await apiCall('open_url', { url });
}

/**
 * Open an external URL with opener-plugin and location fallbacks.
 */
export async function openExternal(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch (err) {
    if (isLocalhostHttpUrl(url)) {
      console.warn('[Shell] Loopback OAuth callback unavailable — not opening browser:', err);
      return;
    }
    console.error('[Shell] openUrl failed:', err);
    if (isTauri()) {
      try {
        const { openUrl: pluginOpenUrl } = await import('@tauri-apps/plugin-opener');
        await pluginOpenUrl(url);
      } catch (pluginErr) {
        console.error('[Shell] plugin-opener failed:', pluginErr);
      }
      return;
    }
    window.location.href = url;
  }
}

/**
 * Perform a native window control action (desktop title bar only).
 */
export async function performWindowControl(action: WindowControlAction): Promise<void> {
  if (!isTauri()) {
    return;
  }
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  const appWindow = getCurrentWindow();
  if (action === 'minimize') {
    appWindow.minimize();
  } else if (action === 'maximize') {
    appWindow.toggleMaximize();
  } else {
    appWindow.close();
  }
}

/**
 * Subscribe to a Tauri event only in the desktop shell.
 */
export async function listenWhenTauri<T>(
  event: string,
  handler: (event: Event<T>) => void
): Promise<UnlistenFn | undefined> {
  if (!isTauri()) {
    return undefined;
  }
  return listen(event, handler);
}

/**
 * Convert an absolute filesystem path to a webview-safe asset URL (desktop only).
 */
export function fileSrcFromAbsolutePath(absolutePath: string | null): string | null {
  if (!absolutePath || !isTauri()) {
    return null;
  }
  return convertFileSrc(absolutePath);
}

/**
 * Open the native file/directory picker (desktop only).
 */
export async function pickPath(
  options: OpenDialogOptions
): Promise<string | string[] | null> {
  if (!isTauri()) {
    return null;
  }
  const selected = await open(options);
  if (selected === null) {
    return null;
  }
  return selected;
}

/**
 * Flush a cold-start OAuth deep link after the consent listener is ready (desktop only).
 */
export async function flushPendingDeepLink(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke('flush_pending_deep_link');
}

/** Payload for OAuth consent deep links on desktop. */
export interface OAuthConsentDeepLinkPayload {
  requestId: string;
}

/**
 * Subscribe to OAuth consent deep-link events and flush any buffered URL (desktop only).
 */
export async function subscribeOAuthConsentRequest(
  handler: (payload: OAuthConsentDeepLinkPayload) => void
): Promise<UnlistenFn | undefined> {
  if (!isTauri()) {
    console.log('[OAuth] subscribeOAuthConsentRequest skipped — not Tauri');
    return undefined;
  }
  console.log('[OAuth] Subscribing to Tauri event:', 'oauth-consent-request');
  const unlisten = await listen<OAuthConsentDeepLinkPayload>(
    'oauth-consent-request',
    (event) => {
      console.log('[OAuth] Tauri consent event received:', event.payload);
      handler(event.payload);
    }
  );
  void flushPendingDeepLink().catch((err) => {
    console.warn('[OAuth] flush_pending_deep_link failed:', err);
  });
  return unlisten;
}

/**
 * Subscribe to OAuth consent requests (Tauri events on desktop, SSE on web admin).
 */
export function subscribeOAuthConsentEvents(
  handler: (payload: OAuthConsentDeepLinkPayload) => void
): () => void {
  if (isTauri()) {
    console.log('[OAuth] subscribeOAuthConsentEvents: using Tauri listener');
    let unlisten: UnlistenFn | undefined;
    void subscribeOAuthConsentRequest(handler).then((fn) => {
      unlisten = fn;
      console.log('[OAuth] Tauri consent listener registered');
    });
    return () => {
      console.log('[OAuth] Unsubscribing Tauri consent listener');
      unlisten?.();
    };
  }

  console.log('[OAuth] subscribeOAuthConsentEvents: using SSE /api/v1/events');
  const source = new EventSource('/api/v1/events');
  source.onopen = () => console.log('[OAuth] SSE connected');
  source.onerror = (err) => console.warn('[OAuth] SSE error:', err);
  const onConsentRequest = (event: MessageEvent<string>) => {
    console.log('[OAuth] SSE consent event raw:', event.data);
    try {
      const payload = JSON.parse(event.data) as OAuthConsentDeepLinkPayload;
      if (payload.requestId) {
        console.log('[OAuth] SSE consent event parsed:', payload);
        handler(payload);
      }
    } catch {
      console.warn('[OAuth] SSE consent event: malformed payload');
    }
  };
  source.addEventListener('oauth-consent-request', onConsentRequest);
  return () => {
    console.log('[OAuth] Closing SSE consent listener');
    source.removeEventListener('oauth-consent-request', onConsentRequest);
    source.close();
  };
}

/**
 * Open the application logs folder in the system file manager (desktop only).
 */
export async function openLogsFolder(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke('open_logs_folder');
}

/**
 * Load web admin HTTP server settings (desktop control plane only).
 */
export async function getAdminWebSettings(): Promise<AdminWebSettings> {
  return invoke('get_admin_web_settings');
}

/**
 * Persist web admin settings and restart the admin HTTP server (desktop only).
 */
export async function updateAdminWebSettings(settings: AdminWebSettings): Promise<void> {
  await invoke('update_admin_web_settings', { settings });
}

/**
 * Reveal a space config file in the system editor (desktop only).
 */
export async function openSpaceConfigFile(spaceId: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke('open_space_config_file', { spaceId });
}

/**
 * Add McpMux to VS Code via deep link (desktop only).
 */
export async function addToVscode(gatewayUrl: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke('add_to_vscode', { gatewayUrl });
}

/**
 * Add McpMux to Cursor via deep link (desktop only).
 */
export async function addToCursor(gatewayUrl: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await invoke('add_to_cursor', { gatewayUrl });
}

/**
 * Write generated MCP client config JSON to a user-selected path (desktop only).
 */
export async function exportConfigToFile(
  request: ExportConfigRequest,
  path: string
): Promise<string> {
  return invoke('export_config_to_file', { request, path });
}

/**
 * Check the Tauri updater for an available release (desktop only).
 */
export async function checkForAvailableUpdate(): Promise<{ version: string } | null> {
  if (!isTauri()) {
    return null;
  }
  const { check } = await import('@tauri-apps/plugin-updater');
  const update = await check();
  if (!update) {
    return null;
  }
  return { version: update.version };
}

/**
 * Run the Tauri updater check and return the full update handle (desktop only).
 */
export async function checkAppUpdate(): Promise<Update | null> {
  if (!isTauri()) {
    return null;
  }
  const { check } = await import('@tauri-apps/plugin-updater');
  return check();
}

/**
 * Relaunch the desktop app after installing an update (desktop only).
 */
export async function relaunchApp(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  await relaunch();
}
