/**
 * Known client name patterns mapped to icon keys.
 * Sorted by specificity (longer patterns first) so that more specific
 * names like "claude desktop" match before the shorter "claude".
 */
const KNOWN_CLIENT_PATTERNS: { pattern: string; iconKey: string }[] = [
  { pattern: 'visual studio code', iconKey: 'vscode' },
  { pattern: 'claude desktop', iconKey: 'claude' },
  { pattern: 'claude code', iconKey: 'claude' },
  { pattern: 'vs code', iconKey: 'vscode' },
  { pattern: 'windsurf', iconKey: 'windsurf' },
  { pattern: 'codeium', iconKey: 'windsurf' },
  { pattern: 'cursor', iconKey: 'cursor' },
  { pattern: 'vscode', iconKey: 'vscode' },
  { pattern: 'claude', iconKey: 'claude' },
];

/**
 * Resolves a client name to a known icon key.
 *
 * Handles exact matches ("Cursor") as well as names that include extra
 * context such as "Claude Code (mcpmux)" — the parenthesised suffix
 * is ignored as long as the prefix matches a known client pattern.
 *
 * Returns the icon key (e.g. "claude", "cursor") or null when the name
 * is not recognised.
 */
export function resolveKnownClientKey(clientName: string): string | null {
  const normalized = clientName.toLowerCase().trim();

  for (const { pattern, iconKey } of KNOWN_CLIENT_PATTERNS) {
    if (normalized === pattern) {
      return iconKey;
    }
    // Accept prefix match only when followed by a word boundary (' ' or '(')
    if (
      normalized.startsWith(pattern) &&
      (normalized[pattern.length] === ' ' || normalized[pattern.length] === '(')
    ) {
      return iconKey;
    }
  }

  return null;
}
