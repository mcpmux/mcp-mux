import { forwardRef, type InputHTMLAttributes } from 'react';
import { Search, X, type LucideIcon } from 'lucide-react';
import { cn } from '../../lib/cn';

export interface SearchFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> {
  onClear?: () => void;
  icon?: LucideIcon;
  'data-testid'?: string;
}

/**
 * Search input with leading icon and optional clear control.
 */
export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(
  ({ className, value, onClear, icon: Icon = Search, 'data-testid': testId, ...props }, ref) => {
    const hasValue = String(value ?? '').length > 0;

    return (
      <div className={cn('relative min-w-0', className)}>
        <Icon className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))] pointer-events-none" />
        <input
          ref={ref}
          type="text"
          value={value}
          data-testid={testId}
          className={cn(
            'w-full pl-9 py-2.5 text-sm bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-lg',
            'focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 transition-all',
            'placeholder:text-[rgb(var(--muted))]',
            hasValue ? 'pr-9' : 'pr-3'
          )}
          {...props}
        />
        {hasValue && onClear && (
          <button
            type="button"
            onClick={onClear}
            className="absolute right-2 top-1/2 -translate-y-1/2 p-1 rounded-md text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
            aria-label="Clear search"
            data-testid={testId ? `${testId}-clear` : undefined}
          >
            <X className="h-4 w-4" />
          </button>
        )}
      </div>
    );
  }
);

SearchField.displayName = 'SearchField';
