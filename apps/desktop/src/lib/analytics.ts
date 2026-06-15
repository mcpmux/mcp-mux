/**
 * PostHog analytics for McpMux desktop app.
 *
 * - Anonymous by default (no PII collected)
 * - Opt-out toggle available in Settings
 * - PostHog auto-captures geolocation from IP
 */

import posthog from 'posthog-js';

const POSTHOG_KEY = import.meta.env.VITE_POSTHOG_KEY ?? '';
const POSTHOG_HOST = import.meta.env.VITE_POSTHOG_HOST ?? 'https://us.i.posthog.com';

let initialized = false;

/** Initialize PostHog with app-level super properties. */
export function initAnalytics(appVersion: string) {
  if (initialized || typeof window === 'undefined') return;
  if (!POSTHOG_KEY) {
    // The #1 reason no events show up in local dev: the key isn't loaded.
    // Vite only reads env at startup and only exposes `VITE_`-prefixed vars,
    // so a missing/empty key here means restart dev or check apps/desktop/.env*.
    console.info('[analytics] disabled — VITE_POSTHOG_KEY is not set; events are dropped');
    return;
  }

  posthog.init(POSTHOG_KEY, {
    api_host: POSTHOG_HOST,
    person_profiles: 'identified_only',
    capture_pageview: false,
    capture_pageleave: false,
    autocapture: false,
    persistence: 'localStorage',
  });

  // Super properties sent with every event
  posthog.register({
    app_version: appVersion,
    os: getOS(),
    platform: 'desktop',
  });

  initialized = true;
  console.info(`[analytics] initialized (host=${POSTHOG_HOST})`);
}

/** Capture an analytics event (no-op if not initialized or opted out). */
export function capture(event: string, properties?: Record<string, unknown>) {
  if (!initialized) {
    // Dev aid: surface dropped events so a missing key / un-run init is obvious
    // when debugging "why isn't <event> in PostHog?".
    if (import.meta.env.DEV) {
      console.debug(`[analytics] dropped "${event}" — analytics not initialized`);
    }
    return;
  }
  if (posthog.has_opted_out_capturing()) {
    if (import.meta.env.DEV) {
      console.debug(`[analytics] dropped "${event}" — user opted out of analytics`);
    }
    return;
  }
  posthog.capture(event, properties);
}

/** Opt out of analytics. */
export function optOut() {
  if (!initialized) return;
  posthog.opt_out_capturing();
}

/** Opt back in to analytics. */
export function optIn() {
  if (!initialized) return;
  posthog.opt_in_capturing();
}

/** Check if user has opted out. */
export function hasOptedOut(): boolean {
  if (!initialized) return false;
  return posthog.has_opted_out_capturing();
}

function getOS(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('win')) return 'windows';
  if (ua.includes('mac')) return 'macos';
  if (ua.includes('linux')) return 'linux';
  return 'unknown';
}
