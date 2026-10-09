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

/**
 * Where approving a consent request sends the user back to, in words the user
 * can check: a loopback callback is "this computer", an https callback is its
 * host, and an app callback is its `scheme://host`.
 */
export function describeRedirectTarget(redirectUri: string): string {
  let url: URL;
  try {
    url = new URL(redirectUri);
  } catch {
    return redirectUri;
  }
  const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if (url.protocol === 'http:' && loopback) {
    return url.port ? `this computer (port ${url.port})` : 'this computer';
  }
  if (url.protocol === 'https:') {
    return url.host;
  }
  return url.host ? `${url.protocol}//${url.host}` : url.protocol;
}
