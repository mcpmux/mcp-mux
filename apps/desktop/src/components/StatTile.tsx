import { ArrowUpRight } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useNavigateTo } from '@/stores';
import type { NavItem } from '@/stores/types';

export interface StatTileProps {
  testId: string;
  valueTestId: string;
  icon: LucideIcon;
  label: string;
  sub: string;
  value: string;
  /** Solid accent for the strip + icon tint. */
  accent: string;
  navTarget: NavItem;
  navHint: string;
}

/**
 * Clickable stat summary with a colored accent strip.
 */
export function StatTile({
  testId,
  valueTestId,
  icon: Icon,
  label,
  sub,
  value,
  accent,
  navTarget,
  navHint,
}: StatTileProps) {
  const navigateTo = useNavigateTo();
  return (
    <button
      type="button"
      onClick={() => navigateTo(navTarget)}
      title={navHint}
      data-testid={testId}
      className="group relative overflow-hidden rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--card))] p-4 text-left shadow transition-all duration-200 hover:-translate-y-0.5 hover:border-[rgb(var(--border))] hover:shadow-md"
    >
      <span
        aria-hidden
        className="absolute inset-y-0 left-0 w-1"
        style={{ backgroundColor: accent }}
      />
      <div className="flex items-start justify-between gap-2 pl-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-medium text-[rgb(var(--muted))]">
            <span
              className="flex h-7 w-7 items-center justify-center rounded-lg"
              style={{ backgroundColor: `color-mix(in srgb, ${accent} 14%, transparent)` }}
            >
              <Icon className="h-4 w-4" style={{ color: accent }} />
            </span>
            {label}
          </div>
          <div
            className="mt-2 truncate text-3xl font-bold tracking-tight"
            data-testid={valueTestId}
          >
            {value}
          </div>
          <div className="mt-0.5 text-xs text-[rgb(var(--muted))]">{sub}</div>
        </div>
        <ArrowUpRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted-foreground))] opacity-0 transition-opacity duration-150 group-hover:opacity-100" />
      </div>
    </button>
  );
}
