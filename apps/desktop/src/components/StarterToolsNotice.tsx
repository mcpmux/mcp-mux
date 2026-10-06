/**
 * Tells the user what their apps get from the Starter and, past the size
 * warning, how to slim it down. The Starter keeps serving every tool either
 * way — this is guidance, not a limit.
 */
import { AlertTriangle, ArrowRight, Layers, Plus } from 'lucide-react';
import type { StarterToolSummary } from '@/lib/api/featureSets';
import { useGoCreateFeatureSet } from '@/hooks/useCrossTabNav';
import { useNavigateTo, type NavItem } from '@/stores';
import { MuxPromptCode } from './MuxPrompt';

function plural(n: number, word: string) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

interface OverThresholdProps {
  summary: StarterToolSummary;
  spaceId: string;
  returnTo: NavItem;
  testId?: string;
}

/**
 * Amber warning once the Starter serves more tools than AI apps handle well.
 * Offers both ways out: ask `@mux`, or build a FeatureSet by hand.
 */
export function StarterOverThresholdWarning({
  summary,
  spaceId,
  returnTo,
  testId = 'starter-tools-warning',
}: OverThresholdProps) {
  const goCreate = useGoCreateFeatureSet(returnTo);
  return (
    <div
      className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 dark:border-amber-700/60 dark:bg-amber-900/20"
      data-testid={testId}
    >
      <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold text-amber-900 dark:text-amber-100">
          Your apps are getting {plural(summary.tool_count, 'tool')} — that&apos;s a lot
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-amber-800 dark:text-amber-200">
          Everything still works, but past {summary.threshold} tools AI apps get slower and pick the
          wrong tool more often, and some apps cap how many they load. Give each project just what
          it needs — ask your AI app:
        </p>
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
          <MuxPromptCode testId={`${testId}-copy`} />
          <span className="text-xs text-amber-800 dark:text-amber-200">or</span>
          <button
            type="button"
            onClick={() => goCreate(spaceId)}
            className="inline-flex items-center gap-1 text-xs font-semibold text-amber-900 hover:underline dark:text-amber-100"
            data-testid={`${testId}-create`}
          >
            <Plus className="h-3 w-3" />
            Create a FeatureSet
          </button>
        </div>
      </div>
    </div>
  );
}

interface HomeCardProps {
  summary: StarterToolSummary;
  spaceId: string;
}

/**
 * Home card: "what do my apps get?" in one line, or the warning when the
 * Starter has grown past the threshold.
 */
export function StarterToolsCard({ summary, spaceId }: HomeCardProps) {
  const navigateTo = useNavigateTo();
  if (summary.over_threshold) {
    return <StarterOverThresholdWarning summary={summary} spaceId={spaceId} returnTo="home" />;
  }
  if (summary.tool_count === 0) return null;

  const detail = summary.auto_include
    ? 'Every tool from every server, through your Starter set — new servers show up on their own.'
    : 'The tools you picked for your Starter set.';
  return (
    <button
      type="button"
      onClick={() => navigateTo('featuresets')}
      className="group flex w-full items-center gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:border-[rgb(var(--border))] hover:shadow-md"
      data-testid="starter-tools-card"
    >
      <span className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
        <Layers className="h-5 w-5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-semibold" data-testid="starter-tools-card-title">
          Your apps get {plural(summary.tool_count, 'tool')} from{' '}
          {plural(summary.server_count, 'server')}
        </span>
        <span className="block text-xs text-[rgb(var(--muted))]">{detail}</span>
      </span>
      <ArrowRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] transition-transform group-hover:translate-x-0.5" />
    </button>
  );
}
