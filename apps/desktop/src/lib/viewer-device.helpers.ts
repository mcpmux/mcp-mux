import { isTauri } from '@/lib/backend/data/transport';

const VIEWER_ID_KEY = 'mcpmux.viewer_device_id';
const VIEWER_NAME_KEY = 'mcpmux.viewer_device_name';
const VIEWER_MACHINE_ID_KEY = 'mcpmux.viewer_machine_id';

/**
 * True when the UI runs on the same host as the gateway (desktop app or localhost admin).
 */
export function isViewingLocally(): boolean {
  if (isTauri()) {
    return true;
  }
  if (typeof window === 'undefined') {
    return false;
  }
  return ['localhost', '127.0.0.1', '[::1]', ''].includes(window.location.hostname);
}

interface NavigatorUADataLite {
  brands: readonly { brand: string; version: string }[];
  platform: string;
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }>;
}

/**
 * Read User-Agent Client Hints when the browser exposes them.
 */
function getNavigatorUAData(): NavigatorUADataLite | null {
  const nav = navigator as Navigator & { userAgentData?: NavigatorUADataLite };
  return nav.userAgentData ?? null;
}

/**
 * Return a stable viewer device id, creating one in localStorage when missing.
 */
export function getOrCreateViewerDeviceId(): string {
  if (typeof window === 'undefined' || !window.localStorage) {
    return crypto.randomUUID();
  }

  const existing = localStorage.getItem(VIEWER_ID_KEY);
  if (existing) {
    return existing;
  }

  const id = crypto.randomUUID();
  localStorage.setItem(VIEWER_ID_KEY, id);
  return id;
}

/**
 * Read the cached machine catalog id for this viewer profile, if set.
 */
export function getViewerMachineIdCache(): string | null {
  if (typeof window === 'undefined' || !window.localStorage) {
    return null;
  }
  const id = localStorage.getItem(VIEWER_MACHINE_ID_KEY)?.trim();
  return id || null;
}

/**
 * Cache the machine catalog id for this viewer profile.
 */
export function setViewerMachineIdCache(machineId: string): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  localStorage.setItem(VIEWER_MACHINE_ID_KEY, machineId);
}

/**
 * Clear the cached machine catalog id for this viewer profile.
 */
export function clearViewerMachineIdCache(): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  localStorage.removeItem(VIEWER_MACHINE_ID_KEY);
}

/**
 * Read the user-assigned display name for this browser profile, if set.
 * Legacy localStorage only; prefer the linked machine row after migration.
 */
export function getViewerDeviceName(): string | null {
  if (typeof window === 'undefined' || !window.localStorage) {
    return null;
  }
  const name = localStorage.getItem(VIEWER_NAME_KEY)?.trim();
  return name || null;
}

/**
 * Persist the viewer display name for this browser profile.
 * @deprecated Use machine catalog rows via viewer identity hook instead.
 */
export function setViewerDeviceName(name: string): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  localStorage.setItem(VIEWER_NAME_KEY, name.trim());
}

/**
 * Drop legacy viewer name storage after migrating to a machine row.
 */
export function clearViewerDeviceName(): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  localStorage.removeItem(VIEWER_NAME_KEY);
}

/**
 * Build a short human-readable subtitle from Client Hints or userAgent fallback.
 */
export async function getViewerDeviceHints(): Promise<string | null> {
  if (typeof navigator === 'undefined') {
    return null;
  }

  const uaData = getNavigatorUAData();
  if (!uaData) {
    return parseUserAgentFallback(navigator.userAgent);
  }

  const brand = pickPrimaryBrand(uaData.brands);
  const parts = [brand, uaData.platform].filter(Boolean);

  if (uaData.getHighEntropyValues) {
    try {
      const hi = await uaData.getHighEntropyValues(['architecture']);
      if (hi.architecture) {
        parts.push(hi.architecture);
      }
    } catch {
      /* high-entropy hints are optional */
    }
  }

  return parts.length > 0 ? parts.join(' · ') : null;
}

/**
 * Pick the primary browser brand from Client Hints brand list.
 */
function pickPrimaryBrand(brands: readonly { brand: string; version: string }[]): string {
  const primary = brands.find((entry) => !/not\s*a/i.test(entry.brand));
  return (primary?.brand ?? brands[0]?.brand ?? 'Browser').replace(/^Google /, '');
}

/**
 * Best-effort platform label when Client Hints are unavailable.
 */
function parseUserAgentFallback(userAgent: string): string | null {
  if (/Macintosh|Mac OS X/i.test(userAgent)) return 'macOS';
  if (/Windows/i.test(userAgent)) return 'Windows';
  if (/Linux/i.test(userAgent)) return 'Linux';
  if (/Android/i.test(userAgent)) return 'Android';
  if (/iPhone|iPad/i.test(userAgent)) return 'iOS';
  return null;
}
