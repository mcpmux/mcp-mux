import { openUrl } from '@/lib/api/gateway';
import { openUrl as openUrlWithPlugin } from '@tauri-apps/plugin-opener';

/**
 * Hand an OAuth redirect (with the code or the error) back to the client app.
 *
 * The redirect target comes from the client's registration, so it is never
 * loaded in this window: it goes to the `open_url` command, falling back to the
 * opener plugin (http/https/mailto/tel only). If both refuse, the error
 * propagates to the caller instead of navigating the webview.
 */
export async function openRedirectUrl(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch (err) {
    console.error('[OAuth] openUrl failed:', err);
    await openUrlWithPlugin(url);
  }
}
