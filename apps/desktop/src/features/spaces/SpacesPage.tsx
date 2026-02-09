import { useState } from 'react';
import {
  Plus,
  Trash2,
  Loader2,
  Check,
  Search,
  Layout,
  AlertCircle,
} from 'lucide-react';
import {
  Card,
  CardHeader,
  CardTitle,
  CardContent,
  Button,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import {
  useAppStore,
  useActiveSpace,
  useSpaces,
  useIsLoading,
} from '@/stores';
import { createSpace, deleteSpace, setActiveSpace as setActiveSpaceAPI } from '@/lib/api/spaces';

export function SpacesPage() {
  const spaces = useSpaces();
  const activeSpace = useActiveSpace();
  const isLoading = useIsLoading('spaces');
  
  // Store actions
  const addSpace = useAppStore((state) => state.addSpace);
  const removeSpace = useAppStore((state) => state.removeSpace);
  const setActiveSpaceInStore = useAppStore((state) => state.setActiveSpace);

  // Local state
  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isActionLoading, setIsActionLoading] = useState<string | null>(null); // ID of space being acted on
  const { toasts, success, error: showError, dismiss } = useToast();

  // Create Modal State
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newSpaceName, setNewSpaceName] = useState('');
  const [newSpaceIcon, setNewSpaceIcon] = useState('🌐');
  const [isCreating, setIsCreating] = useState(false);

  const handleCreate = async () => {
    if (!newSpaceName.trim()) return;
    
    setIsCreating(true);
    setError(null);
    try {
      const space = await createSpace(newSpaceName.trim(), newSpaceIcon);
      addSpace(space);
      setNewSpaceName('');
      setNewSpaceIcon('🌐');
      setShowCreateModal(false);
      success('Space created', `"${space.name}" has been created`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to create space', msg);
    } finally {
      setIsCreating(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Are you sure you want to delete this space? This action cannot be undone.')) return;
    
    setIsActionLoading(id);
    setError(null);
    try {
      const deletedSpace = spaces.find(s => s.id === id);
      await deleteSpace(id);
      removeSpace(id);
      success('Space deleted', `"${deletedSpace?.name || 'Space'}" has been deleted`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to delete space', msg);
    } finally {
      setIsActionLoading(null);
    }
  };

  const handleSetActive = async (id: string) => {
    setIsActionLoading(id);
    setError(null);
    try {
      await setActiveSpaceAPI(id);
      setActiveSpaceInStore(id);
      const activatedSpace = spaces.find(s => s.id === id);
      success('Active space changed', `"${activatedSpace?.name || 'Space'}" is now active`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to set active space', msg);
    } finally {
      setIsActionLoading(null);
    }
  };

  // Filter spaces
  const filteredSpaces = spaces.filter(space => {
    if (!searchQuery) return true;
    const query = searchQuery.toLowerCase();
    return (
      space.name.toLowerCase().includes(query) ||
      (space.description || '').toLowerCase().includes(query)
    );
  });

  return (
    <>
    <ToastContainer toasts={toasts} onClose={dismiss} />
    <div className="h-full flex flex-col relative" data-testid="spaces-page">
      {/* Header */}
      <div className="flex-shrink-0 p-8 border-b border-[rgb(var(--border-subtle))]">
        <div className="max-w-[2000px] mx-auto">
          <div className="flex items-center justify-between mb-6">
            <div>
              <h1 className="text-3xl font-bold">Workspaces</h1>
              <p className="text-base text-[rgb(var(--muted))] mt-2">
                Manage isolated environments with their own credentials and server configurations
              </p>
            </div>
            <Button variant="primary" size="md" onClick={() => setShowCreateModal(true)} data-testid="create-space-btn">
              <Plus className="h-4 w-4 mr-2" />
              Create Space
            </Button>
          </div>

          {/* Search Bar */}
          <div className="relative max-w-3xl">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
            <input
              type="text"
              placeholder="Search workspaces..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full pl-12 pr-4 py-3 text-base bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-xl focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 transition-all"
            />
          </div>
        </div>
      </div>

      {/* Error Banner */}
      {error && (
        <div className="flex-shrink-0 px-8 pt-6">
          <div className="max-w-[2000px] mx-auto p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-xl flex items-start gap-3">
            <AlertCircle className="h-5 w-5 text-red-600 dark:text-red-400 flex-shrink-0 mt-0.5" />
            <p className="text-base text-red-600 dark:text-red-400">{error}</p>
          </div>
        </div>
      )}

      {/* Content Grid */}
      <div className="flex-1 overflow-auto px-8 py-8">
        <div className="max-w-[2000px] mx-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-64">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : filteredSpaces.length === 0 ? (
            <Card className="max-w-2xl mx-auto">
              <CardContent className="flex flex-col items-center justify-center py-16">
                <Layout className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
                <h3 className="text-lg font-medium mb-2">
                  {searchQuery ? 'No spaces match your search' : 'No spaces created'}
                </h3>
                <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
                  {searchQuery 
                    ? 'Try adjusting your search terms' 
                    : 'Create a workspace to isolate your MCP server configurations and credentials.'
                  }
                </p>
                {!searchQuery && (
                  <Button variant="primary" onClick={() => setShowCreateModal(true)}>
                    <Plus className="h-4 w-4 mr-2" />
                    Create First Space
                  </Button>
                )}
              </CardContent>
            </Card>
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filteredSpaces.map((space) => {
                const isActive = activeSpace?.id === space.id;
                const isProcessing = isActionLoading === space.id;

                return (
                  <Card 
                    key={space.id}
                    className={`transition-all hover:shadow-lg hover:scale-[1.01] ${
                      isActive ? 'ring-2 ring-primary-500 shadow-lg' : ''
                    }`}
                    data-testid={`space-card-${space.id}`}
                  >
                    <CardContent className="p-4">
                      {/* Header */}
                      <div className="flex items-start gap-2.5 mb-3">
                        <div className="w-9 h-9 flex items-center justify-center bg-[rgb(var(--surface))] rounded-lg text-xl border border-[rgb(var(--border-subtle))] flex-shrink-0">
                          {space.icon || '🌐'}
                        </div>
                        <div className="flex-1 min-w-0">
                          <h3 className="font-semibold text-base truncate">
                            {space.name}
                          </h3>
                          <p className="text-sm text-[rgb(var(--muted))] line-clamp-1">
                            {space.description || 'No description'}
                          </p>
                        </div>
                        <div className="flex gap-1 flex-shrink-0">
                          {isActive && (
                            <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400">
                              <Check className="h-3 w-3" /> Active
                            </span>
                          )}
                          {!space.is_default && (
                             <button
                               onClick={() => handleDelete(space.id)}
                               disabled={isProcessing || isActive}
                               className="p-1.5 text-[rgb(var(--muted))] hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                               title="Delete Space"
                               data-testid={`delete-space-${space.id}`}
                             >
                               <Trash2 className="h-4 w-4" />
                             </button>
                          )}
                        </div>
                      </div>

                      {/* Footer Actions */}
                      <div className="flex items-center justify-end pt-3 border-t border-[rgb(var(--border-subtle))]">
                         {!isActive ? (
                           <Button
                             size="sm"
                             variant="secondary"
                             onClick={() => handleSetActive(space.id)}
                             disabled={isProcessing}
                             data-testid={`set-active-space-${space.id}`}
                           >
                             {isProcessing ? (
                               <Loader2 className="h-3 w-3 animate-spin mr-2" />
                             ) : null}
                             Set Active
                           </Button>
                         ) : (
                           <span className="text-xs font-medium text-[rgb(var(--muted))]">
                             Current Context
                           </span>
                         )}
                      </div>
                    </CardContent>
                  </Card>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Create Modal */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" data-testid="create-space-modal-overlay">
          <Card className="w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-200 shadow-2xl" data-testid="create-space-modal">
            <CardHeader>
              <CardTitle className="flex items-center justify-between">
                <span className="flex items-center gap-2">
                  <Plus className="h-5 w-5" />
                  Create Workspace
                </span>
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="p-1 rounded hover:bg-[rgb(var(--surface-hover))]"
                >
                  <XIcon className="h-4 w-4" />
                </button>
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div>
                <label className="block text-sm font-medium mb-1.5">Icon</label>
                <div className="flex gap-2 overflow-x-auto p-1 pb-2">
                   {['🌐', '💻', '🚀', '🏢', '🏠', '🔒', '🧪', '📦'].map(icon => (
                     <button
                       key={icon}
                       onClick={() => setNewSpaceIcon(icon)}
                       className={`w-10 h-10 flex items-center justify-center rounded-lg text-xl border transition-all ${
                         newSpaceIcon === icon
                           ? 'bg-primary-50 dark:bg-primary-900/20 border-primary-500 ring-2 ring-primary-500/20'
                           : 'bg-[rgb(var(--surface))] border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))]'
                       }`}
                     >
                       {icon}
                     </button>
                   ))}
                </div>
              </div>

              <div>
                <label className="block text-sm font-medium mb-1.5">Name *</label>
                <input
                  type="text"
                  value={newSpaceName}
                  onChange={(e) => setNewSpaceName(e.target.value)}
                  placeholder="e.g., Personal, Work, Project X"
                  className="w-full px-3 py-2.5 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                  autoFocus
                  data-testid="create-space-name-input"
                />
              </div>

              <div className="pt-2 flex gap-3">
                <Button 
                  variant="ghost" 
                  onClick={() => setShowCreateModal(false)}
                  className="flex-1"
                  data-testid="create-space-cancel-btn"
                >
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  onClick={handleCreate}
                  disabled={isCreating || !newSpaceName.trim()}
                  className="flex-1"
                  data-testid="create-space-submit-btn"
                >
                  {isCreating ? <Loader2 className="h-4 w-4 animate-spin mr-2" /> : 'Create Space'}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
    </>
  );
}

// Helper component for X icon to avoid import conflict or missing import
function XIcon({ className }: { className?: string }) {
  return (
    <svg 
      xmlns="http://www.w3.org/2000/svg" 
      width="24" 
      height="24" 
      viewBox="0 0 24 24" 
      fill="none" 
      stroke="currentColor" 
      strokeWidth="2" 
      strokeLinecap="round" 
      strokeLinejoin="round" 
      className={className}
    >
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </svg>
  );
}

export default SpacesPage;
