/** RFC 4122 UUID v1–v5 (case-insensitive). */
const MACHINE_UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

/**
 * True when value is a non-empty machine catalog UUID after trim.
 */
export function isMachineUuid(value: string): boolean {
  const trimmed = value.trim();
  return trimmed.length > 0 && MACHINE_UUID_RE.test(trimmed);
}

/**
 * Build the MCP client config snippet for per-device machine identity.
 */
export function buildMcpMachineHeaderSnippet(machineId: string): string {
  return JSON.stringify({ headers: { 'X-Mcpmux-Machine-Id': machineId.trim() } }, null, 2);
}

/**
 * Write text to the system clipboard.
 */
export async function copyTextToClipboard(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}
