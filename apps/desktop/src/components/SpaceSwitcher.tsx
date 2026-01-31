import { useState, useRef, useEffect } from 'react';
import {
  ChevronDown,
  Check,
  Plus,
  Loader2,
} from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  useAppStore,
  useActiveSpace,
  useViewSpace,
  useSpaces,
  useIsLoading,
} from '@/stores';
import { createSpace, setActiveSpace as setActiveSpaceAPI } from '@/lib/api/spaces';

interface SpaceSwitcherProps {
  className?: string;
}

export function SpaceSwitcher({ className = '' }: SpaceSwitcherProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [showCreateInput, setShowCreateInput] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const spaces = useSpaces();
  const activeSpace = useActiveSpace();
  const viewSpace = useViewSpace();
  const isLoadingSpaces = useIsLoading('spaces');
  const setActiveSpaceInStore = useAppStore((state) => state.setActiveSpace);
  const setViewSpaceInStore = useAppStore((state) => state.setViewSpace);
  const addSpace = useAppStore((state) => state.addSpace);

  // Close dropdown when clicking outside
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

  const handleSetActiveSpace = async (spaceId: string) => {
    try {
      await setActiveSpaceAPI(spaceId);
      setActiveSpaceInStore(spaceId);
      setIsOpen(false);
    } catch (e) {
      console.error('Failed to switch space:', e);
    }
  };

  const handleCreateSpace = async () => {
    if (!newName.trim()) return;
    setIsCreating(true);
    try {
      const space = await createSpace(newName.trim(), '🌐');
      addSpace(space);
      await setActiveSpaceAPI(space.id);
      setActiveSpaceInStore(space.id);
      setViewSpaceInStore(space.id);
      setNewName('');
      setShowCreateInput(false);
      setIsOpen(false);
    } catch (e) {
      console.error('Failed to create space:', e);
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div ref={dropdownRef} className={`relative ${className}`}>
      {/* Trigger Button */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="w-full flex items-center justify-between gap-2 px-3 py-2.5 rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))] hover:border-[rgb(var(--primary))/30] transition-all duration-150 group"
      >
        <span className="flex items-center gap-3 min-w-0">
          {isLoadingSpaces ? (
            <Loader2 className="h-5 w-5 animate-spin text-[rgb(var(--primary))]" />
          ) : (
            <span className="text-xl">{viewSpace?.icon || '🌐'}</span>
          )}
          <span className="font-medium text-sm truncate">
            {isLoadingSpaces 
              ? 'Loading...' 
              : viewSpace?.name || (spaces.length > 0 ? 'Select Space' : 'No Spaces')
            }
          </span>
        </span>
        <ChevronDown className={`h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))] group-hover:text-[rgb(var(--foreground))] transition-all duration-200 ${isOpen ? 'rotate-180' : ''}`} />
      </button>

      {/* Dropdown */}
      {isOpen && (
        <div className="dropdown-menu absolute top-full left-0 mt-1.5 w-64 z-50 animate-in fade-in slide-in-from-top-1 duration-150">
          {/* Spaces List */}
          <div className="p-1.5 max-h-64 overflow-y-auto">
            {isLoadingSpaces ? (
              <div className="flex items-center justify-center py-4">
                <Loader2 className="h-5 w-5 animate-spin text-primary-500" />
                <span className="ml-2 text-sm text-[rgb(var(--muted))]">Loading spaces...</span>
              </div>
            ) : spaces.length === 0 ? (
              <div className="text-center py-4 text-sm text-[rgb(var(--muted))]">
                No spaces found. Create one below.
              </div>
            ) : (
              spaces.map((space) => (
                <button
                  key={space.id}
                  onClick={() => handleSelectSpace(space.id)}
                  className={`w-full flex items-center justify-between px-3 py-2.5 rounded-lg text-left transition-all duration-150
                    ${viewSpace?.id === space.id 
                      ? 'bg-[rgb(var(--primary))/12] text-[rgb(var(--primary))]' 
                      : 'hover:bg-[rgb(var(--surface-hover))]'
                    }`}
                >
                  <span className="flex items-center gap-3">
                    <span className="text-xl">{space.icon || '🌐'}</span>
                    <div>
                      <div className="font-medium text-sm">{space.name}</div>
                      {space.is_default && (
                        <div className="text-xs text-[rgb(var(--muted))]">Default</div>
                      )}
                    </div>
                  </span>
                  <span className="flex items-center gap-2">
                    {activeSpace?.id === space.id && (
                      <span className="text-xs text-[rgb(var(--muted))]">Active</span>
                    )}
                    {viewSpace?.id === space.id && (
                      <Check className="h-4 w-4" />
                    )}
                    {activeSpace?.id !== space.id && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleSetActiveSpace(space.id);
                        }}
                      >
                        Set Active
                      </Button>
                    )}
                  </span>
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
                className="w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))] transition-all duration-150"
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
