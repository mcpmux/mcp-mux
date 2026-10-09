/** Longest argument dump the dialog renders. */
const MAX_ARGS_CHARS = 4000;

/** The client's raw arguments as indented JSON, capped at `MAX_ARGS_CHARS`. */
export function formatRawArgs(args: unknown): string {
  let text: string;
  try {
    text = JSON.stringify(args ?? null, null, 2) ?? 'null';
  } catch {
    text = String(args);
  }
  return text.length > MAX_ARGS_CHARS ? `${text.slice(0, MAX_ARGS_CHARS)}\n…` : text;
}
