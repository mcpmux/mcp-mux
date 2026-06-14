import { useState } from 'react';
import { Plus, Trash2, Loader2, Search, Layout, AlertCircle } from 'lucide-react';
import { Card, CardContent, Button, useToast, ToastContainer, useConfirm } from '@mcpmux/ui';
import { useAppStore, useSpaces, useIsLoading } from '@/stores';
import { deleteSpace } from '@/lib/api/spaces';
import { CreateSpaceModal } from './CreateSpaceModal';

export function SpacesPage() {
  const spaces = useSpaces();
  const isLoading = useIsLoading('spaces');

  // Store actions
  const removeSpace = useAppStore((state) => state.removeSpace);

  // Local state
  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isActionLoading, setIsActionLoading] = useState<string | null>(null); // ID of space being acted on
  const { confirm, ConfirmDialogElement } = useConfirm();
  const { toasts, success, error: showError, dismiss } = useToast();

  // Create Modal State (creation logic lives in the shared CreateSpaceModal)
  const [showCreateModal, setShowCreateModal] = useState(false);

  const handleDelete = async (id: string) => {
    const spaceName = spaces.find((s) => s.id === id)?.name || 'this space';
    if (
      !(await confirm({
        title: 'Delete workspace',
        message: `Are you sure you want to delete "${spaceName}"? This action cannot be undone.`,
        confirmLabel: 'Delete',
        variant: 'danger',
      }))
    )
      return;

    setIsActionLoading(id);
    setError(null);
    try {
      const deletedSpace = spaces.find((s) => s.id === id);
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

  // Filter spaces
  const filteredSpaces = spaces.filter((space) => {
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
      {ConfirmDialogElement}
      <div className="relative flex h-full flex-col" data-testid="spaces-page">
        {/* Header */}
        <div className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
          <div className="mx-auto max-w-[2000px]">
            <div className="mb-6 flex items-center justify-between">
              <div>
                <h1 className="text-3xl font-bold">Spaces</h1>
                <p className="mt-2 max-w-2xl text-base text-[rgb(var(--muted))]">
                  Isolated contexts — work, personal, per client — each with its own servers,
                  credentials, and tool bundles
                </p>
              </div>
              <Button
                variant="primary"
                size="md"
                onClick={() => setShowCreateModal(true)}
                data-testid="create-space-btn"
              >
                <Plus className="mr-2 h-4 w-4" />
                Create Space
              </Button>
            </div>

            {/* Search Bar */}
            <div className="relative max-w-3xl">
              <Search className="absolute left-4 top-1/2 h-5 w-5 -translate-y-1/2 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder="Search spaces..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="focus:ring-primary-500 focus:border-primary-500 w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] py-3 pl-12 pr-4 text-base transition-all focus:outline-none focus:ring-2"
              />
            </div>
          </div>
        </div>

        {/* Error Banner */}
        {error && (
          <div className="flex-shrink-0 px-8 pt-6">
            <div className="mx-auto flex max-w-[2000px] items-start gap-3 rounded-xl border border-red-200 bg-red-50 p-4 dark:border-red-800 dark:bg-red-900/20">
              <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0 text-red-600 dark:text-red-400" />
              <p className="text-base text-red-600 dark:text-red-400">{error}</p>
            </div>
          </div>
        )}

        {/* Content Grid */}
        <div className="flex-1 overflow-auto px-8 py-8">
          <div className="mx-auto max-w-[2000px]">
            {isLoading ? (
              <div className="flex h-64 items-center justify-center">
                <Loader2 className="text-primary-500 h-8 w-8 animate-spin" />
              </div>
            ) : filteredSpaces.length === 0 ? (
              <Card className="mx-auto max-w-2xl">
                <CardContent className="flex flex-col items-center justify-center py-16">
                  <Layout className="mb-4 h-16 w-16 text-[rgb(var(--muted))]" />
                  <h3 className="mb-2 text-lg font-medium">
                    {searchQuery ? 'No spaces match your search' : 'No spaces created'}
                  </h3>
                  <p className="mb-6 max-w-md text-center text-sm text-[rgb(var(--muted))]">
                    {searchQuery
                      ? 'Try adjusting your search terms'
                      : 'Create a workspace to isolate your MCP server configurations and credentials.'}
                  </p>
                  {!searchQuery && (
                    <Button variant="primary" onClick={() => setShowCreateModal(true)}>
                      <Plus className="mr-2 h-4 w-4" />
                      Create First Space
                    </Button>
                  )}
                </CardContent>
              </Card>
            ) : (
              <div className="auto-fill-cards grid gap-5">
                {filteredSpaces.map((space) => {
                  const isProcessing = isActionLoading === space.id;

                  return (
                    <Card
                      key={space.id}
                      className="transition-all hover:scale-[1.01] hover:shadow-lg"
                      data-testid={`space-card-${space.id}`}
                    >
                      <CardContent className="p-4">
                        <div className="mb-3 flex items-start gap-2.5">
                          <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] text-xl">
                            {space.icon || '🌐'}
                          </div>
                          <div className="min-w-0 flex-1">
                            <h3 className="truncate text-base font-semibold">{space.name}</h3>
                            <p className="line-clamp-1 text-sm text-[rgb(var(--muted))]">
                              {space.description || 'No description'}
                            </p>
                          </div>
                          <div className="flex flex-shrink-0 gap-1">
                            {space.is_default && (
                              <span
                                className="inline-flex items-center gap-1 rounded-full bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-700 dark:bg-blue-900/30 dark:text-blue-400"
                                title="Default home for sessions whose reported root has no binding"
                              >
                                Default
                              </span>
                            )}
                            {!space.is_default && (
                              <button
                                onClick={() => handleDelete(space.id)}
                                disabled={isProcessing}
                                className="rounded-lg p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-red-50 hover:text-red-500 disabled:cursor-not-allowed disabled:opacity-50 dark:hover:bg-red-900/20"
                                title="Delete Space"
                                data-testid={`delete-space-${space.id}`}
                              >
                                <Trash2 className="h-4 w-4" />
                              </button>
                            )}
                          </div>
                        </div>
                      </CardContent>
                    </Card>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        <CreateSpaceModal open={showCreateModal} onClose={() => setShowCreateModal(false)} />
      </div>
    </>
  );
}

export default SpacesPage;
