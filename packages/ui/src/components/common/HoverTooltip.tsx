import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '../../lib/cn';

export type HoverTooltipSide = 'top' | 'bottom' | 'auto';

const VIEWPORT_PADDING = 8;
const GAP = 8;

export interface HoverTooltipProps {
  children: ReactNode;
  title: string;
  lines?: string[];
  /** Preferred placement; `auto` flips based on available viewport space. */
  side?: HoverTooltipSide;
  className?: string;
  hidden?: boolean;
  'data-testid'?: string;
}

/**
 * Pick top or bottom placement from viewport space around the trigger.
 */
function resolveTooltipSide(
  preferred: HoverTooltipSide,
  triggerRect: DOMRect,
  tooltipHeight: number
): 'top' | 'bottom' {
  const spaceAbove = triggerRect.top;
  const spaceBelow = window.innerHeight - triggerRect.bottom;
  const needed = tooltipHeight + GAP;

  if (preferred === 'top') {
    if (spaceAbove >= needed) {
      return 'top';
    }
    if (spaceBelow >= needed) {
      return 'bottom';
    }
    return spaceBelow > spaceAbove ? 'bottom' : 'top';
  }

  if (preferred === 'bottom') {
    if (spaceBelow >= needed) {
      return 'bottom';
    }
    if (spaceAbove >= needed) {
      return 'top';
    }
    return spaceAbove > spaceBelow ? 'top' : 'bottom';
  }

  if (spaceAbove >= needed && spaceBelow >= needed) {
    return spaceAbove >= spaceBelow ? 'top' : 'bottom';
  }
  if (spaceBelow >= needed) {
    return 'bottom';
  }
  if (spaceAbove >= needed) {
    return 'top';
  }
  return spaceBelow > spaceAbove ? 'bottom' : 'top';
}

/**
 * Compute fixed viewport coordinates for the tooltip panel.
 */
function computeTooltipCoords(
  triggerRect: DOMRect,
  tooltipWidth: number,
  tooltipHeight: number,
  placement: 'top' | 'bottom'
): { top: number; left: number } {
  let top =
    placement === 'top'
      ? triggerRect.top - tooltipHeight - GAP
      : triggerRect.bottom + GAP;

  top = Math.max(
    VIEWPORT_PADDING,
    Math.min(top, window.innerHeight - tooltipHeight - VIEWPORT_PADDING)
  );

  let left = triggerRect.right - tooltipWidth;
  left = Math.max(
    VIEWPORT_PADDING,
    Math.min(left, window.innerWidth - tooltipWidth - VIEWPORT_PADDING)
  );

  return { top, left };
}

/**
 * Wraps a control and shows a tooltip panel on hover (hidden while `hidden` is true).
 * Placement flips above/below based on viewport space when `side` is `auto`.
 */
export function HoverTooltip({
  children,
  title,
  lines = [],
  side = 'auto',
  className,
  hidden = false,
  'data-testid': testId,
}: HoverTooltipProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(false);
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null);

  const updateCoords = useCallback(() => {
    const container = containerRef.current;
    const tooltip = tooltipRef.current;
    if (!container || !tooltip) {
      return;
    }

    const triggerRect = container.getBoundingClientRect();
    const tooltipRect = tooltip.getBoundingClientRect();
    const tooltipWidth = tooltipRect.width > 0 ? tooltipRect.width : tooltip.scrollWidth;
    const tooltipHeight = tooltipRect.height > 0 ? tooltipRect.height : tooltip.scrollHeight;

    const placement = resolveTooltipSide(side, triggerRect, tooltipHeight);
    setCoords(computeTooltipCoords(triggerRect, tooltipWidth, tooltipHeight, placement));
  }, [side]);

  useLayoutEffect(() => {
    if (!active || hidden) {
      return;
    }
    updateCoords();
  }, [active, hidden, updateCoords, title, lines]);

  useEffect(() => {
    if (!active || hidden) {
      return;
    }

    const handleReposition = () => updateCoords();
    window.addEventListener('resize', handleReposition);
    window.addEventListener('scroll', handleReposition, true);
    return () => {
      window.removeEventListener('resize', handleReposition);
      window.removeEventListener('scroll', handleReposition, true);
    };
  }, [active, hidden, updateCoords]);

  const showCoords = active && !hidden ? coords : null;
  const showTooltip = showCoords !== null;

  return (
    <div
      ref={containerRef}
      className={cn('relative', className)}
      onMouseEnter={() => setActive(true)}
      onMouseLeave={() => setActive(false)}
      onFocusCapture={() => setActive(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setActive(false);
        }
      }}
    >
      <div
        ref={tooltipRef}
        role="tooltip"
        className={cn(
          'pointer-events-none fixed z-[60] min-w-[10rem] max-w-xs dropdown-menu px-3 py-2 text-left',
          'transition-opacity duration-150',
          showTooltip ? 'opacity-100' : 'opacity-0'
        )}
        style={
          showCoords
            ? { top: showCoords.top, left: showCoords.left }
            : { top: -9999, left: -9999, visibility: 'hidden' as const }
        }
        data-testid={testId}
      >
        <p className="text-xs font-medium text-[rgb(var(--foreground))] mb-1">{title}</p>
        {lines.map((line) => (
          <p key={line} className="text-xs text-[rgb(var(--muted))] leading-relaxed">
            {line}
          </p>
        ))}
      </div>
      {children}
    </div>
  );
}
