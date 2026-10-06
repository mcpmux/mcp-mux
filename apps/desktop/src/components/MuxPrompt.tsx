/**
 * The `@mux` prompt — how a user asks their own AI app to shape their toolset
 * (find the tools a project needs, compose a FeatureSet, map it to the
 * folder). Shown wherever that help is relevant: Mapping, FeatureSets, the
 * tool-count warning, and the status bar. Every surface teaches the same
 * copyable prompt so users learn one thing.
 */
import { Check, Copy, Sparkles } from 'lucide-react';
import { MUX_OPTIMIZE_PROMPT, useCopyPrompt } from '@/hooks/useCopyPrompt';

interface MuxPromptCodeProps {
  prompt?: string;
  testId?: string;
}

/** The prompt as a code chip with a copy button. */
export function MuxPromptCode({ prompt = MUX_OPTIMIZE_PROMPT, testId }: MuxPromptCodeProps) {
  const { copied, copy } = useCopyPrompt(prompt);
  return (
    <span className="inline-flex max-w-full items-center gap-1 rounded-md border border-violet-200/80 bg-[rgb(var(--surface))] py-0.5 pl-2 pr-0.5 align-middle dark:border-violet-800/50">
      <code className="truncate font-mono text-[11px] text-violet-700 dark:text-violet-300">
        {prompt}
      </code>
      <button
        type="button"
        onClick={copy}
        className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded text-[rgb(var(--muted))] transition-colors hover:bg-violet-500/10 hover:text-violet-600 dark:hover:text-violet-300"
        title={copied ? 'Copied' : 'Copy prompt'}
        aria-label={copied ? 'Copied' : 'Copy prompt'}
        data-testid={testId}
      >
        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
      </button>
    </span>
  );
}

interface MuxPromptBannerProps {
  title: string;
  children: React.ReactNode;
  prompt?: string;
  testId?: string;
}

/**
 * Full-width violet banner: a title, one or two lines of context, and the
 * copyable prompt.
 */
export function MuxPromptBanner({ title, children, prompt, testId }: MuxPromptBannerProps) {
  return (
    <div
      className="flex items-start gap-3 rounded-xl border border-violet-200/70 bg-gradient-to-r from-violet-50/60 to-transparent p-4 dark:border-violet-800/40 dark:from-violet-900/15"
      data-testid={testId}
    >
      <div className="mt-0.5 flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-violet-500 to-fuchsia-500 text-white shadow-[0_4px_10px_-2px_rgb(139_92_246/0.45)]">
        <Sparkles className="h-4 w-4 fill-current" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold text-[rgb(var(--foreground))]">{title}</p>
        <div className="mt-0.5 text-xs leading-relaxed text-[rgb(var(--muted))]">{children}</div>
        <div className="mt-2">
          <MuxPromptCode prompt={prompt} testId={testId ? `${testId}-copy` : undefined} />
        </div>
      </div>
    </div>
  );
}

/**
 * Compact status-bar button: one click copies the prompt, the tooltip says
 * what `@mux` does.
 */
export function MuxStatusChip() {
  const { copied, copy } = useCopyPrompt();
  return (
    <button
      type="button"
      onClick={copy}
      className="flex items-center gap-1 transition-colors hover:text-violet-600 dark:hover:text-violet-300"
      title={`Ask your AI app to curate its tools. Click to copy: "${MUX_OPTIMIZE_PROMPT}"`}
      data-testid="statusbar-mux"
    >
      <Sparkles className="h-3 w-3" />
      {copied ? 'Prompt copied' : '@mux'}
    </button>
  );
}
