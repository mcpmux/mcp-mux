/** Local timezone for build/commit display (matches generAIt frontend). */
const BUILD_TIMEZONE = 'America/Denver';

const buildDateFormatter = new Intl.DateTimeFormat('en-US', {
  timeZone: BUILD_TIMEZONE,
  weekday: 'short',
  month: 'short',
  day: 'numeric',
  year: 'numeric',
});

const buildTimeFormatter = new Intl.DateTimeFormat('en-US', {
  timeZone: BUILD_TIMEZONE,
  hour: 'numeric',
  minute: '2-digit',
  second: '2-digit',
  hour12: true,
  timeZoneName: 'short',
});

/**
 * Format a date for build metadata (e.g. "Wed, May 29 2026").
 */
export function formatBuildDate(date: Date): string {
  return buildDateFormatter.format(date);
}

/**
 * Format a time for build metadata (e.g. "06:32:19 PM MDT").
 */
export function formatBuildTime(date: Date): string {
  return buildTimeFormatter.format(date);
}

/**
 * generAIt-style combined stamp: "Wed, May 29 2026 at 06:32:19 PM MDT".
 */
export function formatBuiltAt(date: Date): string {
  return `${formatBuildDate(date)} at ${formatBuildTime(date)}`;
}

/**
 * Normalize git `%ci` (`YYYY-MM-DD HH:MM:SS ±HHMM`) to ISO 8601 for cross-engine parsing.
 */
function normalizeStampInstant(raw: string): string {
  const gitCi = /^(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2}:\d{2}) ([+-])(\d{2})(\d{2})$/.exec(raw);
  if (gitCi) {
    const [, date, time, sign, hours, minutes] = gitCi;
    return `${date}T${time}${sign}${hours}:${minutes}`;
  }
  return raw;
}

/**
 * Parse git `%ci` commit time or Rust UTC build strings.
 */
export function parseStampInstant(raw: string): Date | null {
  const trimmed = raw.trim();
  if (!trimmed || trimmed === 'unknown') {
    return null;
  }
  if (trimmed.endsWith(' UTC')) {
    const iso = trimmed.replace(' UTC', 'Z').replace(' ', 'T');
    const parsed = Date.parse(iso);
    return Number.isNaN(parsed) ? null : new Date(parsed);
  }
  const parsed = Date.parse(normalizeStampInstant(trimmed));
  return Number.isNaN(parsed) ? null : new Date(parsed);
}

/**
 * Format a raw git/Rust timestamp for display.
 */
export function formatStampInstant(raw: string, fallback = 'unknown'): string {
  const date = parseStampInstant(raw);
  return date ? formatBuiltAt(date) : fallback;
}
