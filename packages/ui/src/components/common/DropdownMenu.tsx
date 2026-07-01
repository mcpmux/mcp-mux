import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode,
} from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '../../lib/cn';
import { useClickOutside } from '../../hooks/useClickOutside';

interface DropdownMenuContextValue {
  open: boolean;
  setOpen: (open: boolean) => void;
  menuId: string;
}

const DropdownMenuContext = createContext<DropdownMenuContextValue | null>(null);

function useDropdownMenu(): DropdownMenuContextValue {
  const context = useContext(DropdownMenuContext);
  if (!context) {
    throw new Error('DropdownMenu components must be used within DropdownMenu');
  }
  return context;
}

export interface DropdownMenuProps {
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}

/**
 * Root dropdown container with open state and click-outside handling.
 */
export function DropdownMenu({
  children,
  open: controlledOpen,
  onOpenChange,
  className,
}: DropdownMenuProps) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  const open = controlledOpen ?? uncontrolledOpen;

  const setOpen = useCallback(
    (next: boolean) => {
      if (controlledOpen === undefined) {
        setUncontrolledOpen(next);
      }
      onOpenChange?.(next);
    },
    [controlledOpen, onOpenChange]
  );

  useClickOutside([rootRef], () => setOpen(false), open);

  useEffect(() => {
    if (!open) {
      return;
    }

    function handleEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setOpen(false);
      }
    }

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [open, setOpen]);

  return (
    <DropdownMenuContext.Provider value={{ open, setOpen, menuId }}>
      <div ref={rootRef} className={cn('relative inline-flex', className)}>
        {children}
      </div>
    </DropdownMenuContext.Provider>
  );
}

export interface DropdownMenuTriggerProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

/**
 * Wraps the element that toggles the dropdown open state.
 */
export function DropdownMenuTrigger({ children, className, ...props }: DropdownMenuTriggerProps) {
  const { open, setOpen, menuId } = useDropdownMenu();

  return (
    <div
      className={cn('inline-flex', className)}
      onClick={() => setOpen(!open)}
      aria-expanded={open}
      aria-haspopup="menu"
      aria-controls={menuId}
      {...props}
    >
      {children}
    </div>
  );
}

export interface DropdownMenuContentProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  align?: 'start' | 'end';
}

/**
 * Panel shown below the trigger when the menu is open.
 */
export function DropdownMenuContent({
  children,
  align = 'end',
  className,
  ...props
}: DropdownMenuContentProps) {
  const { open, menuId } = useDropdownMenu();

  if (!open) {
    return null;
  }

  return (
    <div
      id={menuId}
      role="menu"
      className={cn(
        'dropdown-menu absolute top-full mt-1.5 z-50 animate-in fade-in slide-in-from-top-1 duration-150',
        align === 'end' ? 'right-0' : 'left-0',
        className
      )}
      {...props}
    >
      {children}
    </div>
  );
}

export interface DropdownMenuItemProps {
  icon?: LucideIcon;
  label: string;
  description?: string;
  onSelect: () => void;
  variant?: 'default' | 'warning' | 'danger';
  className?: string;
  'data-testid'?: string;
}

/**
 * Menu row with optional icon, title, and description (for discover/custom style items).
 */
export function DropdownMenuItem({
  icon: Icon,
  label,
  description,
  onSelect,
  variant = 'default',
  className,
  'data-testid': testId,
}: DropdownMenuItemProps) {
  const { setOpen } = useDropdownMenu();

  const labelClass =
    variant === 'danger'
      ? 'text-[rgb(var(--error))]'
      : variant === 'warning'
        ? 'text-[rgb(var(--warning))]'
        : 'text-[rgb(var(--foreground))]';

  return (
    <button
      type="button"
      role="menuitem"
      onClick={() => {
        setOpen(false);
        onSelect();
      }}
      className={cn(
        'w-full text-left flex items-start gap-3 px-3 py-2.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors',
        variant === 'danger' && 'hover:bg-[rgb(var(--error))]/10',
        className
      )}
      data-testid={testId}
    >
      {Icon && (
        <Icon
          className={cn(
            'h-4 w-4 mt-0.5 flex-shrink-0',
            variant === 'default' ? 'text-[rgb(var(--muted))]' : labelClass
          )}
        />
      )}
      <div className="flex-1 min-w-0">
        <p className={cn('text-sm font-medium', labelClass)}>{label}</p>
        {description && (
          <p className="text-xs text-[rgb(var(--muted))] mt-0.5 leading-relaxed">{description}</p>
        )}
      </div>
    </button>
  );
}

/**
 * Simple compact menu row (icon + label) for action menus.
 */
export function DropdownMenuAction({
  icon: Icon,
  label,
  onSelect,
  variant = 'default',
  className,
  'data-testid': testId,
}: Omit<DropdownMenuItemProps, 'description'>) {
  const { setOpen } = useDropdownMenu();

  const labelClass =
    variant === 'danger'
      ? 'text-[rgb(var(--error))]'
      : variant === 'warning'
        ? 'text-[rgb(var(--warning))]'
        : 'text-[rgb(var(--foreground))]';

  return (
    <button
      type="button"
      role="menuitem"
      onClick={() => {
        setOpen(false);
        onSelect();
      }}
      className={cn(
        'w-full flex items-center gap-2 px-3 py-2 text-sm hover:bg-[rgb(var(--surface-hover))] transition-colors',
        variant === 'danger' && 'hover:bg-[rgb(var(--error))]/10',
        className
      )}
      data-testid={testId}
    >
      {Icon && (
        <Icon className={cn('h-4 w-4', variant === 'default' ? 'text-[rgb(var(--muted))]' : labelClass)} />
      )}
      <span className={labelClass}>{label}</span>
    </button>
  );
}

export function DropdownMenuSeparator() {
  return <div className="my-1 border-t border-[rgb(var(--border-subtle))]" role="separator" />;
}
