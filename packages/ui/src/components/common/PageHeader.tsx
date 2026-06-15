import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface PageHeaderProps {
  title: string;
  /** Newcomer-friendly one-or-two sentence description. Plain string or rich nodes. */
  subtitle?: ReactNode;
  /** Right-aligned actions (buttons, menus). */
  actions?: ReactNode;
  /** Inline badge next to the title (e.g. "Offline", counts). */
  badge?: ReactNode;
  className?: string;
  'data-testid'?: string;
  /** testid applied to the h1 so existing *-title selectors keep working. */
  titleTestId?: string;
}

/**
 * The one page-header pattern: bold tracking-tight title, muted subtitle
 * capped at a readable width, actions pinned right. Every page adopts this
 * so new surfaces (Chat, Agents, Models) inherit a consistent shell.
 */
export function PageHeader({
  title,
  subtitle,
  actions,
  badge,
  className,
  'data-testid': testId,
  titleTestId,
}: PageHeaderProps) {
  return (
    <div
      className={cn('mb-6 flex items-start justify-between gap-4', className)}
      data-testid={testId}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold tracking-tight" data-testid={titleTestId}>
            {title}
          </h1>
          {badge}
        </div>
        {subtitle && (
          <p className="mt-1.5 max-w-2xl text-sm leading-relaxed text-[rgb(var(--muted))]">
            {subtitle}
          </p>
        )}
      </div>
      {actions && <div className="flex flex-shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}
