import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface SidebarProps {
  children: ReactNode;
  header?: ReactNode;
  footer?: ReactNode;
}

export function Sidebar({ children, header, footer }: SidebarProps) {
  return (
    <nav className="flex h-full flex-col bg-[rgb(var(--surface))]">
      {header && (
        <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border-subtle))]">
          {header}
        </div>
      )}
      <div className="flex-1 overflow-y-auto p-3">{children}</div>
      {footer && (
        <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border-subtle))]">
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
}

export function SidebarItem({ icon, label, active, onClick }: SidebarItemProps) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all duration-150',
        'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))]',
        active && 'bg-[rgb(var(--primary))/10] text-[rgb(var(--primary))] hover:bg-[rgb(var(--primary))/15]'
      )}
    >
      {icon && (
        <span className={cn(
          'h-5 w-5 flex items-center justify-center transition-colors',
          active && 'text-[rgb(var(--primary))]'
        )}>
          {icon}
        </span>
      )}
      <span>{label}</span>
    </button>
  );
}

interface SidebarSectionProps {
  title?: string;
  children: ReactNode;
}

export function SidebarSection({ title, children }: SidebarSectionProps) {
  return (
    <div className="mb-6">
      {title && (
        <h3 className="mb-2 px-3 text-[11px] font-semibold uppercase tracking-wider text-[rgb(var(--muted-foreground))]">
          {title}
        </h3>
      )}
      <div className="space-y-0.5">{children}</div>
    </div>
  );
}

