import { type ButtonHTMLAttributes, forwardRef } from 'react';
import { cn } from '../../lib/cn';

export type ChipButtonVariant = 'fill' | 'outline';

export interface ChipButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  variant?: ChipButtonVariant;
}

/**
 * Small pill toggle used for transport/status filter chips.
 */
export const ChipButton = forwardRef<HTMLButtonElement, ChipButtonProps>(
  ({ className, active = false, variant = 'fill', children, type = 'button', ...props }, ref) => {
    return (
      <button
        ref={ref}
        type={type}
        className={cn(
          'px-3 py-1.5 text-xs font-medium rounded-lg border transition-all',
          active && variant === 'fill' && [
            'bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] border-[rgb(var(--primary))]',
          ],
          active &&
            variant === 'outline' && [
              'bg-[rgb(var(--primary))]/15 text-[rgb(var(--primary))] border-[rgb(var(--primary))]/40',
              'ring-1 ring-[rgb(var(--primary))]/30',
            ],
          !active && [
            'bg-[rgb(var(--surface-elevated))] text-[rgb(var(--muted))] border-[rgb(var(--border))]',
            'hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))]',
          ],
          className
        )}
        {...props}
      >
        {children}
      </button>
    );
  }
);

ChipButton.displayName = 'ChipButton';
