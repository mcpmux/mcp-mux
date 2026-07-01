import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface SidebarProps {
  children: ReactNode;
  header?: ReactNode;
  footer?: ReactNode;
}

export function Sidebar({ children, header, footer }: SidebarProps) {
  return (
    <nav className="flex h-full flex-col bg-[rgb(var(--surface))]" data-testid="sidebar">
      {header && (
        <div className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-4">
          {header}
        </div>
      )}
      <div className="flex-1 overflow-y-auto px-3 py-3">{children}</div>
      {footer && (
        <div className="flex-shrink-0 border-t border-[rgb(var(--border-subtle))] px-3 py-3">
          {footer}
        </div>
      )}
    </nav>
  );
}

interface SidebarItemProps {
  icon?: ReactNode;
  label: string;
  active?: boolean;
  onClick?: () => void;
  /** One-line tooltip explaining the destination. */
  hint?: string;
  /** Native tooltip override (e.g. alias label → canonical name). */
  title?: string;
  'data-testid'?: string;
}

export function SidebarItem({
  icon,
  label,
  active,
  onClick,
  hint,
  title,
  'data-testid': testId,
}: SidebarItemProps) {
  return (
    <button
      onClick={onClick}
      data-testid={testId}
      title={title ?? hint}
      className={cn(
        'group relative flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium',
        'transition-all duration-150',
        'text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))]',
        active &&
          'bg-[rgb(var(--primary))/10] text-[rgb(var(--primary))] hover:bg-[rgb(var(--primary))/15] hover:text-[rgb(var(--primary))]'
      )}
    >
      {/* Accent strip — the app-wide "selected" language (solid bar on the left). */}
      <span
        aria-hidden
        className={cn(
          'absolute left-0 top-1/2 h-5 w-[3px] -translate-y-1/2 rounded-full bg-[rgb(var(--primary))]',
          'transition-all duration-200',
          active ? 'scale-y-100 opacity-100' : 'scale-y-50 opacity-0'
        )}
      />
      {icon && (
        <span
          className={cn(
            'flex h-5 w-5 items-center justify-center transition-transform duration-150',
            'group-hover:scale-105',
            active && 'text-[rgb(var(--primary))]'
          )}
        >
          {icon}
        </span>
      )}
      <span className="truncate">{label}</span>
    </button>
  );
}

interface SidebarSectionProps {
  title?: string;
  children: ReactNode;
}

export function SidebarSection({ title, children }: SidebarSectionProps) {
  return (
    <div className="mb-5 last:mb-0">
      {title && (
        <h3 className="mb-1.5 select-none px-3 text-[10px] font-semibold uppercase tracking-[0.14em] text-[rgb(var(--muted-foreground))]">
          {title}
        </h3>
      )}
      <div className="space-y-0.5">{children}</div>
    </div>
  );
}
