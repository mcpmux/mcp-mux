/**
 * Deterministic per-Space accent color.
 *
 * Each Space gets a stable hue derived from its id, used as a small visual
 * signature (switcher ring, status-bar dot) so users always know which
 * context they're in — the superapp plan's "Space accents" design language.
 *
 * Curated hues only (no full hue wheel): every value reads well on both
 * themes as a soft tint, and none collides with the terracotta primary.
 */

const ACCENT_HUES = [
  199, // sky
  152, // emerald
  262, // violet
  43, // amber
  339, // rose
  174, // teal
  226, // indigo
  84, // lime
] as const;

function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (h << 5) - h + s.charCodeAt(i);
    h |= 0;
  }
  return Math.abs(h);
}

/** Solid accent color for dots/rings/strips. */
export function spaceAccentColor(spaceId: string | undefined | null): string {
  if (!spaceId) return 'hsl(199 65% 52%)';
  const hue = ACCENT_HUES[hashString(spaceId) % ACCENT_HUES.length];
  return `hsl(${hue} 65% 52%)`;
}

/** Soft translucent tint of the same hue, for backgrounds. */
export function spaceAccentTint(spaceId: string | undefined | null, alpha = 0.12): string {
  if (!spaceId) return `hsl(199 65% 52% / ${alpha})`;
  const hue = ACCENT_HUES[hashString(spaceId) % ACCENT_HUES.length];
  return `hsl(${hue} 65% 52% / ${alpha})`;
}
