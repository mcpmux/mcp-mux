import { useCallback, useEffect, useRef, useState } from 'react';

/** The example prompt every `@mux` surface teaches. */
export const MUX_OPTIMIZE_PROMPT = '@mux build a minimal toolset for this project';

/** Copy a prompt to the clipboard; `copied` flips true for a moment. */
export function useCopyPrompt(prompt: string = MUX_OPTIMIZE_PROMPT) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    []
  );

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(prompt);
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1800);
    } catch (err) {
      console.error('Failed to copy prompt:', err);
    }
  }, [prompt]);

  return { copied, copy };
}
