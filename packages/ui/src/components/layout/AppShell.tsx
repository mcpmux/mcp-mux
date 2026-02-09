import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface AppShellProps {
  sidebar: ReactNode;
  children: ReactNode;
  statusBar?: ReactNode;
  titleBar?: ReactNode;
  windowControls?: ReactNode;
  className?: string;
}

export function AppShell({ sidebar, children, statusBar, titleBar, windowControls, className }: AppShellProps) {
  return (
    <div className={cn('flex h-screen flex-col overflow-hidden bg-[rgb(var(--background))]', className)}>
      {/* Custom title bar */}
      {titleBar && (
        <div className="h-9 flex-shrink-0 bg-[rgb(var(--surface))] border-b border-[rgb(var(--border-subtle))] flex items-center">
          {/* Draggable area — fills space between logo and window controls */}
          <div className="drag-region flex-1 h-full flex items-center">
            {titleBar}
          </div>
          {/* Window controls — outside drag region so clicks work */}
          {windowControls}
        </div>
      )}

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
