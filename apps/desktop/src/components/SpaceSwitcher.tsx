import { useState, useRef, useEffect } from 'react';
import { ChevronDown, Check, Plus, Loader2 } from 'lucide-react';
import { Button, useToast, ToastContainer } from '@mcpmux/ui';
import { useAppStore, useViewSpace, useSpaces, useIsLoading } from '@/stores';
import { createSpace } from '@/lib/api/spaces';
import { spaceAccentTint } from '@/lib/spaceAccent';

/** Space icon inside a soft tile tinted with the Space's accent color. */
function SpaceGlyph({
  spaceId,
  icon,
  size = 'md',
}: {
  spaceId: string | undefined;
  icon: string | undefined | null;
  size?: 'md' | 'sm';
}) {
  return (
    <span
      className={`flex flex-shrink-0 items-center justify-center rounded-lg ${
        size === 'md' ? 'h-8 w-8 text-lg' : 'h-7 w-7 text-base'
      }`}
      style={{
        backgroundColor: spaceAccentTint(spaceId, 0.16),
        boxShadow: `inset 0 0 0 1px ${spaceAccentTint(spaceId, 0.35)}`,
      }}
    >
      {icon || '🌐'}
    </span>
  );
}

interface SpaceSwitcherProps {
  className?: string;
}

/**
 * Sidebar dropdown for switching which Space the desktop UI is currently
 * viewing. Pure UI navigation — does not affect gateway routing. The
 * "Default" badge marks the system fallback Space (the one used when a
 * session has no matching WorkspaceBinding).
 */
export function SpaceSwitcher({ className = '' }: SpaceSwitcherProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [showCreateInput, setShowCreateInput] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const { toasts, success, error: showError, dismiss } = useToast();

  const spaces = useSpaces();
  const viewSpace = useViewSpace();
  const isLoadingSpaces = useIsLoading('spaces');
  const setViewSpaceInStore = useAppStore((state) => state.setViewSpace);
  const addSpace = useAppStore((state) => state.addSpace);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
        setShowCreateInput(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSelectSpace = (spaceId: string) => {
    setViewSpaceInStore(spaceId);
    setIsOpen(false);
  };

  const handleCreateSpace = async () => {
    if (!newName.trim()) return;
    setIsCreating(true);
    try {
      const space = await createSpace(newName.trim(), '🌐');
      addSpace(space);
      setViewSpaceInStore(space.id);
      setNewName('');
      setShowCreateInput(false);
      setIsOpen(false);
      success('Space created', `"${space.name}" has been created`);
    } catch (e) {
      showError('Failed to create space', e instanceof Error ? e.message : String(e));
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div ref={dropdownRef} className={`relative ${className}`}>
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {/* Trigger Button */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="group flex w-full items-center justify-between gap-2 rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2.5 transition-all duration-150 hover:border-[rgb(var(--primary))/30] hover:bg-[rgb(var(--surface-hover))]"
      >
        <span className="flex min-w-0 items-center gap-2.5">
          {isLoadingSpaces ? (
            <Loader2 className="h-5 w-5 animate-spin text-[rgb(var(--primary))]" />
          ) : (
            <SpaceGlyph spaceId={viewSpace?.id} icon={viewSpace?.icon} />
          )}
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium">
              {isLoadingSpaces
                ? 'Loading...'
                : viewSpace?.name || (spaces.length > 0 ? 'Select Space' : 'No Spaces')}
            </span>
            {!isLoadingSpaces && viewSpace && (
              <span className="block text-[10px] uppercase tracking-wider text-[rgb(var(--muted-foreground))]">
                Space
              </span>
            )}
          </span>
        </span>
        <ChevronDown
          className={`h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] transition-all duration-200 group-hover:text-[rgb(var(--foreground))] ${isOpen ? 'rotate-180' : ''}`}
        />
      </button>

      {/* Dropdown */}
      {isOpen && (
        <div className="dropdown-menu animate-in fade-in slide-in-from-top-1 absolute left-0 top-full z-50 mt-1.5 w-64 duration-150">
          {/* Spaces List */}
          <div className="max-h-64 overflow-y-auto p-1.5">
            {isLoadingSpaces ? (
              <div className="flex items-center justify-center py-4">
                <Loader2 className="text-primary-500 h-5 w-5 animate-spin" />
                <span className="ml-2 text-sm text-[rgb(var(--muted))]">Loading spaces...</span>
              </div>
            ) : spaces.length === 0 ? (
              <div className="py-4 text-center text-sm text-[rgb(var(--muted))]">
                No spaces found. Create one below.
              </div>
            ) : (
              spaces.map((space) => (
                <button
                  key={space.id}
                  onClick={() => handleSelectSpace(space.id)}
                  className={`flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left transition-all duration-150 ${
                    viewSpace?.id === space.id
                      ? 'bg-[rgb(var(--primary))/12] text-[rgb(var(--primary))]'
                      : 'hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                  data-testid={`space-switcher-item-${space.id}`}
                >
                  <span className="flex min-w-0 items-center gap-3">
                    <SpaceGlyph spaceId={space.id} icon={space.icon} size="sm" />
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium">{space.name}</div>
                      {space.is_default && (
                        <div
                          className="text-xs text-[rgb(var(--muted))]"
                          title="Routing fallback when no WorkspaceBinding matches"
                        >
                          Default
                        </div>
                      )}
                    </div>
                  </span>
                  {viewSpace?.id === space.id && <Check className="h-4 w-4 flex-shrink-0" />}
                </button>
              ))
            )}
          </div>

          {/* Divider */}
          <div className="mx-1.5 border-t border-[rgb(var(--border))]" />

          {/* Create New */}
          <div className="p-1.5">
            {showCreateInput ? (
              <div className="flex gap-2 p-1">
                <input
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="Space name..."
                  autoFocus
                  className="input flex-1 py-1.5"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleCreateSpace();
                    if (e.key === 'Escape') {
                      setShowCreateInput(false);
                      setNewName('');
                    }
                  }}
                />
                <Button
                  size="sm"
                  variant="primary"
                  onClick={handleCreateSpace}
                  disabled={isCreating || !newName.trim()}
                >
                  {isCreating ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Add'}
                </Button>
              </div>
            ) : (
              <button
                onClick={() => setShowCreateInput(true)}
                className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-[rgb(var(--muted))] transition-all duration-150 hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))]"
              >
                <Plus className="h-4 w-4" />
                Create new space
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
