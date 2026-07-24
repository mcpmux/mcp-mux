import { useMemo, useState } from 'react';
import { ChevronDown, Check, Plus } from 'lucide-react';
import { cn } from '../../lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './DropdownMenu';
import { SearchField } from './SearchField';

export interface SearchableSelectOption<T extends string> {
  value: T;
  label: string;
  icon?: string;
}

export interface SearchableSelectProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: SearchableSelectOption<T>[];
  placeholder: string;
  onCreateNew?: () => void;
  disabled?: boolean;
  testId?: string;
}

/**
 * Typeahead-filterable combobox built from DropdownMenu and SearchField.
 */
export function SearchableSelect<T extends string>({
  value,
  onChange,
  options,
  placeholder,
  onCreateNew,
  disabled = false,
  testId,
}: SearchableSelectProps<T>) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState('');

  const selectedOption = options.find((option) => option.value === value);

  const filteredOptions = useMemo(() => {
    const query = filter.trim().toLowerCase();
    if (!query) {
      return options;
    }
    return options.filter((option) => option.label.toLowerCase().includes(query));
  }, [filter, options]);

  /**
   * Sync open state and reset the filter when the panel closes.
   */
  const handleOpenChange = (nextOpen: boolean) => {
    if (disabled && nextOpen) {
      return;
    }
    setOpen(nextOpen);
    if (!nextOpen) {
      setFilter('');
    }
  };

  /**
   * Select an option, notify the parent, and close the panel.
   */
  const handleSelect = (optionValue: T) => {
    onChange(optionValue);
    setOpen(false);
    setFilter('');
  };

  /**
   * Invoke the optional create-new callback and close the panel.
   */
  const handleCreateNew = () => {
    onCreateNew?.();
    setOpen(false);
    setFilter('');
  };

  return (
    <DropdownMenu open={open} onOpenChange={handleOpenChange} className="w-full">
      <DropdownMenuTrigger
        data-testid={testId}
        className={cn(
          'w-full flex items-center justify-between gap-2 px-3 py-2.5 rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] transition-all duration-150 group',
          disabled
            ? 'opacity-50 cursor-not-allowed'
            : 'hover:bg-[rgb(var(--surface-hover))] hover:border-[rgb(var(--primary))/30] cursor-pointer'
        )}
        aria-disabled={disabled}
      >
        <span className="flex items-center gap-3 min-w-0">
          {selectedOption?.icon && (
            <span className="text-xl flex-shrink-0">{selectedOption.icon}</span>
          )}
          <span
            className={cn(
              'font-medium text-sm truncate',
              selectedOption ? 'text-[rgb(var(--foreground))]' : 'text-[rgb(var(--muted))]'
            )}
          >
            {selectedOption?.label ?? placeholder}
          </span>
        </span>
        <ChevronDown
          className={cn(
            'h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] group-hover:text-[rgb(var(--foreground))] transition-all duration-200',
            open && 'rotate-180'
          )}
        />
      </DropdownMenuTrigger>

      <DropdownMenuContent align="start" className="w-full min-w-[16rem]">
        <div className="p-1.5 border-b border-[rgb(var(--border-subtle))]">
          <SearchField
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            onClear={() => setFilter('')}
            placeholder="Search…"
            autoFocus
            data-testid={testId ? `${testId}-search` : undefined}
            onClick={(event) => event.stopPropagation()}
            onKeyDown={(event) => event.stopPropagation()}
          />
        </div>

        <div className="p-1.5 max-h-64 overflow-y-auto">
          {filteredOptions.length === 0 ? (
            <div className="text-center py-4 text-sm text-[rgb(var(--muted))]">No matches</div>
          ) : (
            filteredOptions.map((option) => {
              const isSelected = option.value === value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="menuitem"
                  onClick={() => handleSelect(option.value)}
                  className={cn(
                    'w-full flex items-center justify-between px-3 py-2.5 rounded-lg text-left transition-all duration-150',
                    isSelected
                      ? 'bg-[rgb(var(--primary))/12] text-[rgb(var(--primary))]'
                      : 'hover:bg-[rgb(var(--surface-hover))]'
                  )}
                  data-testid={testId ? `${testId}-option-${option.value}` : undefined}
                >
                  <span className="flex items-center gap-3 min-w-0">
                    {option.icon && <span className="text-xl flex-shrink-0">{option.icon}</span>}
                    <span className="font-medium text-sm truncate">{option.label}</span>
                  </span>
                  {isSelected && <Check className="h-4 w-4 flex-shrink-0" />}
                </button>
              );
            })
          )}
        </div>

        {onCreateNew && (
          <>
            <DropdownMenuSeparator />
            <div className="p-1.5">
              <button
                type="button"
                role="menuitem"
                onClick={handleCreateNew}
                className="w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))] transition-all duration-150"
                data-testid={testId ? `${testId}-create-new` : undefined}
              >
                <Plus className="h-4 w-4" />
                New…
              </button>
            </div>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
