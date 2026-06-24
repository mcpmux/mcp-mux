import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Loader2, Search, Layout, AlertCircle, Pencil } from 'lucide-react';
import {
  Card,
  CardHeader,
  CardTitle,
  CardContent,
  Button,
  useToast,
  ToastContainer,
  useConfirm,
} from '@mcpmux/ui';
import { useAppStore, useSpaces, useIsLoading } from '@/stores';
import { createSpace } from '@/lib/api/spaces';
import { SpacePanel } from './SpacePanel';

/**
 * Spaces management page — list, create, and edit isolated workspace environments.
 */
export function SpacesPage() {
  const { t } = useTranslation(['spaces', 'common']);
  const spaces = useSpaces();
  const isLoading = useIsLoading('spaces');

  const addSpace = useAppStore((state) => state.addSpace);
  const removeSpace = useAppStore((state) => state.removeSpace);
  const updateSpaceInStore = useAppStore((state) => state.updateSpace);

  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const { ConfirmDialogElement } = useConfirm();
  const { toasts, success, error: showError, dismiss } = useToast();

  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newSpaceName, setNewSpaceName] = useState('');
  const [newSpaceIcon, setNewSpaceIcon] = useState('🌐');
  const [isCreating, setIsCreating] = useState(false);
  const [selectedSpaceId, setSelectedSpaceId] = useState<string | null>(null);

  /** Creates a new space from the modal form and adds it to the store. */
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
      success(t('toast.created'), t('toast.createdBody', { name: space.name }));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError(t('toast.createFailed'), msg);
    } finally {
      setIsCreating(false);
    }
  };

  const selectedSpace = selectedSpaceId
    ? spaces.find((s) => s.id === selectedSpaceId) ?? null
    : null;

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
      <div className="h-full flex flex-col relative" data-testid="spaces-page">
        <div className="flex-shrink-0 p-8 border-b border-[rgb(var(--border-subtle))]">
          <div className="max-w-[2000px] mx-auto">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h1 className="text-3xl font-bold" data-testid="spaces-title">
                  {t('title')}
                </h1>
                <p className="text-base text-[rgb(var(--muted))] mt-2">{t('subtitle')}</p>
              </div>
              <Button
                variant="primary"
                size="md"
                onClick={() => setShowCreateModal(true)}
                data-testid="create-space-btn"
              >
                <Plus className="h-4 w-4 mr-2" />
                {t('createSpace')}
              </Button>
            </div>

            <div className="relative max-w-3xl">
              <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder={t('searchPlaceholder')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-12 pr-4 py-3 text-base bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-xl focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 transition-all"
              />
            </div>
          </div>
        </div>

        {error && (
          <div className="flex-shrink-0 px-8 pt-6">
            <div className="max-w-[2000px] mx-auto p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-xl flex items-start gap-3">
              <AlertCircle className="h-5 w-5 text-red-600 dark:text-red-400 flex-shrink-0 mt-0.5" />
              <p className="text-base text-red-600 dark:text-red-400">{error}</p>
            </div>
          </div>
        )}

        <div className="flex-1 overflow-auto px-8 py-8">
          <div className="max-w-[2000px] mx-auto">
            {isLoading ? (
              <div className="flex items-center justify-center h-64">
                <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
              </div>
            ) : filteredSpaces.length === 0 ? (
              <Card
                className="max-w-2xl mx-auto"
                data-testid={searchQuery ? 'spaces-empty-no-match' : 'spaces-empty-state'}
              >
                <CardContent className="flex flex-col items-center justify-center py-16">
                  <Layout className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
                  <h3 className="text-lg font-medium mb-2">
                    {searchQuery ? t('empty.noMatchTitle') : t('empty.noCreatedTitle')}
                  </h3>
                  <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
                    {searchQuery ? t('empty.noMatchDesc') : t('empty.noCreatedDesc')}
                  </p>
                  {!searchQuery && (
                    <Button variant="primary" onClick={() => setShowCreateModal(true)}>
                      <Plus className="h-4 w-4 mr-2" />
                      {t('empty.createFirst')}
                    </Button>
                  )}
                </CardContent>
              </Card>
            ) : (
              <div className="grid gap-5 auto-fill-cards">
                {filteredSpaces.map((space) => {
                  const isSelected = selectedSpaceId === space.id;

                  return (
                    <Card
                      key={space.id}
                      className={`cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] group ${
                        isSelected ? 'ring-2 ring-primary-500 shadow-lg' : ''
                      }`}
                      onClick={() => setSelectedSpaceId(space.id)}
                      data-testid={`space-card-${space.id}`}
                    >
                      <CardContent className="p-4">
                        <div className="flex items-start gap-2.5">
                          <div className="w-9 h-9 flex items-center justify-center bg-[rgb(var(--surface))] rounded-lg text-xl border border-[rgb(var(--border-subtle))] flex-shrink-0">
                            {space.icon || '🌐'}
                          </div>
                          <div className="flex-1 min-w-0">
                            <h3 className="font-semibold text-base truncate">{space.name}</h3>
                            <p className="text-sm text-[rgb(var(--muted))] line-clamp-1">
                              {space.description || t('card.noDescription')}
                            </p>
                          </div>
                          <div className="flex gap-1 flex-shrink-0 items-start">
                            {space.is_default && (
                              <span
                                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400"
                                title={t('card.defaultTitle')}
                              >
                                {t('card.default')}
                              </span>
                            )}
                            <span
                              className="p-1.5 text-[rgb(var(--muted))] opacity-0 group-hover:opacity-100 transition-opacity"
                              title={t('card.editTitle')}
                            >
                              <Pencil className="h-4 w-4" />
                            </span>
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

        {selectedSpace && (
          <>
            <div
              className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
              onClick={() => setSelectedSpaceId(null)}
            />
            <SpacePanel
              space={selectedSpace}
              onClose={() => setSelectedSpaceId(null)}
              onSaved={(updated) => {
                updateSpaceInStore(updated.id, updated);
                setSelectedSpaceId(updated.id);
              }}
              onDeleted={(id) => {
                removeSpace(id);
                setSelectedSpaceId(null);
              }}
            />
          </>
        )}

        {showCreateModal && (
          <div
            className="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
            data-testid="create-space-modal-overlay"
          >
            <Card
              className="w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-200 shadow-2xl"
              data-testid="create-space-modal"
            >
              <CardHeader>
                <CardTitle className="flex items-center justify-between">
                  <span className="flex items-center gap-2">
                    <Plus className="h-5 w-5" />
                    {t('createModal.title')}
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
                  <label className="block text-sm font-medium mb-1.5">{t('createModal.iconLabel')}</label>
                  <div className="flex gap-2 overflow-x-auto p-1 pb-2">
                    {['🌐', '💻', '🚀', '🏢', '🏠', '🔒', '🧪', '📦'].map((icon) => (
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
                  <label className="block text-sm font-medium mb-1.5">{t('createModal.nameLabel')}</label>
                  <input
                    type="text"
                    value={newSpaceName}
                    onChange={(e) => setNewSpaceName(e.target.value)}
                    placeholder={t('createModal.namePlaceholder')}
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
                    {t('common:actions.cancel')}
                  </Button>
                  <Button
                    variant="primary"
                    onClick={handleCreate}
                    disabled={isCreating || !newSpaceName.trim()}
                    className="flex-1"
                    data-testid="create-space-submit-btn"
                  >
                    {isCreating ? (
                      <Loader2 className="h-4 w-4 animate-spin mr-2" />
                    ) : (
                      t('createModal.createSpace')
                    )}
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

/** Inline close icon to avoid lucide import naming conflicts. */
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
