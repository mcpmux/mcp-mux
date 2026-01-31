import { ButtonHTMLAttributes, forwardRef } from 'react';
import { cn } from '../../lib/cn';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'sm' | 'md' | 'lg';
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'primary', size = 'md', children, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          'inline-flex items-center justify-center gap-2 rounded-lg font-medium',
          'transition-all duration-150 ease-out',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(var(--primary))] focus-visible:ring-offset-2',
          'disabled:pointer-events-none disabled:opacity-50',
          'active:scale-[0.98]',
          // Variants
          variant === 'primary' && [
            'bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))]',
            'hover:bg-[rgb(var(--primary-hover))]',
            'shadow-sm hover:shadow',
          ],
          variant === 'secondary' && [
            'bg-[rgb(var(--surface-active))] text-[rgb(var(--foreground))]',
            'hover:bg-[rgb(var(--surface-hover))]',
            'border border-[rgb(var(--border))]',
          ],
          variant === 'ghost' && [
            'text-[rgb(var(--foreground))]',
            'hover:bg-[rgb(var(--surface-hover))]',
          ],
          variant === 'danger' && [
            'bg-[rgb(var(--error))] text-white',
            'hover:bg-[rgb(var(--error))]/90',
            'shadow-sm hover:shadow',
          ],
          // Sizes
          size === 'sm' && 'h-8 px-3 text-sm',
          size === 'md' && 'h-10 px-4',
          size === 'lg' && 'h-12 px-6 text-lg',
          className
        )}
        {...props}
      >
        {children}
      </button>
    );
  }
);

Button.displayName = 'Button';

