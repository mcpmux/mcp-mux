/**
 * Switch/Toggle component
 * A simple toggle switch for boolean settings
 */

import { cn } from '../../lib/cn';

interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
  'data-testid'?: string;
}

export function Switch({
  checked,
  onCheckedChange,
  disabled = false,
  className,
  'data-testid': testId,
}: SwitchProps) {
  const handleClick = () => {
    if (!disabled) {
      onCheckedChange(!checked);
    }
  };

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={handleClick}
      data-testid={testId}
      className={cn(
        'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))] focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50',
        checked 
          ? 'bg-[rgb(var(--primary))] border-transparent' 
          : 'bg-gray-300 dark:bg-gray-600 border-gray-400 dark:border-gray-500',
        className
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow-lg ring-0 transition duration-200 ease-in-out',
          checked ? 'translate-x-5' : 'translate-x-0'
        )}
      />
    </button>
  );
}
