import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface AppShellProps {
  sidebar: ReactNode;
  children: ReactNode;
  statusBar?: ReactNode;
  className?: string;
}

export function AppShell({ sidebar, children, statusBar, className }: AppShellProps) {
  return (
    <div className={cn('flex h-screen flex-col overflow-hidden bg-[rgb(var(--background))]', className)}>
      {/* Title bar (draggable) */}
      <div className="drag-region h-8 flex-shrink-0 bg-[rgb(var(--surface))] border-b border-[rgb(var(--border-subtle))]" />

      {/* Main content */}
      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <aside className="w-60 flex-shrink-0 border-r border-[rgb(var(--border-subtle))]">
          {sidebar}
        </aside>

        {/* Content area */}
        <main className="flex-1 overflow-auto p-6 bg-[rgb(var(--background))]">
          {children}
        </main>
      </div>

      {/* Status bar */}
      {statusBar && (
        <div className="h-7 flex-shrink-0 border-t border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-4">
          {statusBar}
        </div>
      )}
    </div>
  );
}

