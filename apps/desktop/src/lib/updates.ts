import { check, Update } from '@tauri-apps/plugin-updater';

import { isTauri } from '@/lib/backend/data/transport';
import { apiCall } from '@/lib/api/transport';

/**
 * Update channels. "stable" follows published GitHub releases; "prerelease"
 * follows the newest pre-release (and any newer stable). The selection is sent
 * to the update resolver as a request header — Tauri's updater can't template
 * a channel into the endpoint URL, but it does forward custom headers.
 */
export type UpdateChannel = 'stable' | 'prerelease';

/** Header the update resolver reads to pick which channel's manifest to serve. */
export const UPDATE_CHANNEL_HEADER = 'X-Mcpmux-Channel';

/** Read the persisted update channel, defaulting to "stable" if unavailable. */
export async function getUpdateChannel(): Promise<UpdateChannel> {
  try {
    const channel = await apiCall<string>('get_update_channel');
    return channel === 'prerelease' ? 'prerelease' : 'stable';
  } catch {
    return 'stable';
  }
}

/** Persist the update channel. Returns the normalized value actually saved. */
export async function setUpdateChannel(channel: UpdateChannel): Promise<UpdateChannel> {
  const saved = await apiCall<string>('set_update_channel', { channel });
  return saved === 'prerelease' ? 'prerelease' : 'stable';
}

/** Read whether desktop builds should auto-install updates (desktop only). */
export async function getAutoInstallUpdates(): Promise<boolean> {
  if (!isTauri()) {
    return false;
  }
  return apiCall<boolean>('get_auto_install_updates');
}

/**
 * Check for an update on the user's selected channel. Reads the persisted
 * channel and forwards it to the resolver via the {@link UPDATE_CHANNEL_HEADER}
 * header so the same `check()`/`downloadAndInstall()` flow serves both channels.
 */
export async function checkForUpdate(): Promise<Update | null> {
  if (!isTauri()) {
    return null;
  }
  const channel = await getUpdateChannel();
  return check({ headers: { [UPDATE_CHANNEL_HEADER]: channel } });
}
