import { ReactNode } from 'react';
import { cn } from '../../lib/cn';

interface StatusBarProps {
  children: ReactNode;
  className?: string;
}

export function StatusBar({ children, className }: StatusBarProps) {
  return (
    <div
      className={cn(
        'flex h-full items-center justify-between text-xs text-[rgb(var(--muted))]',
        className
      )}
    >
      {children}
    </div>
  );
}

interface StatusBarItemProps {
  children: ReactNode;
  className?: string;
}

export function StatusBarItem({ children, className }: StatusBarItemProps) {
  return (
    <span className={cn('flex items-center gap-1.5', className)}>
      {children}
    </span>
  );
}

