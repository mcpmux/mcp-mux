import { useState, useEffect, useCallback } from 'react';
import {
  Plus,
  Loader2,
  Package,
  Settings,
  X,
  RefreshCw,
  Star,
  Search,
  AlertCircle,
  CheckCircle2,
  Zap,
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
import type { FeatureSet, CreateFeatureSetInput } from '@/lib/api/featureSets';
import {
  listFeatureSetsBySpace,
  createFeatureSet,
  deleteFeatureSet,
  getFeatureSetWithMembers,
  isStarterFeatureSet,
} from '@/lib/api/featureSets';
import { useViewSpace } from '@/stores';
import { FeatureSetPanel } from './FeatureSetPanel';

// Get icon for feature set type
const getFeatureSetIcon = (fs: FeatureSet) => {
  if (fs.icon) return <span className="text-xl">{fs.icon}</span>;

  switch (fs.feature_set_type) {
    case 'starter':
    case 'default': // legacy alias — pre-migration-013 reads still parse here
      return <Star className="h-8 w-8 text-yellow-500" />;
    case 'custom':
    default:
      return <Package className="h-8 w-8 text-purple-500" />;
  }
};

// Get display name for feature set type. The 'default' alias is kept on
// the read path so a stale row from before migration 013 still renders
// the right pill — migration 013 rewrites stored values to 'starter'.
const getFeatureSetTypeName = (type: string) => {
  switch (type) {
    case 'starter':
    case 'default':
      return 'Starter';
    case 'custom':
    default:
      return 'Custom';
  }
};

export function FeatureSetsPage() {
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const viewSpace = useViewSpace();
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const { toasts, success, error: showError } = useToast();

  // Create modal state
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [createName, setCreateName] = useState('');
  const [createDescription, setCreateDescription] = useState('');
  const [createIcon, setCreateIcon] = useState('');

  // Panel state
  const [selectedFeatureSet, setSelectedFeatureSet] = useState<FeatureSet | null>(null);

  const loadData = useCallback(async (spaceId?: string) => {
    setIsLoading(true);
    setError(null);
    try {
      if (!spaceId) {
        setFeatureSets([]);
        return;
      }
      const data = await listFeatureSetsBySpace(spaceId);
      setFeatureSets(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    setSelectedFeatureSet(null);
    setShowCreateModal(false);
    loadData(viewSpace?.id);
  }, [viewSpace?.id, loadData]);

  const handleCreate = async () => {
    if (!createName.trim() || !viewSpace) return;

    setIsCreating(true);
    setError(null);
    try {
      const input: CreateFeatureSetInput = {
        name: createName.trim(),
        space_id: viewSpace.id,
        description: createDescription.trim() || undefined,
        icon: createIcon.trim() || undefined,
      };
      const newFs = await createFeatureSet(input);
      setFeatureSets((prev) => [...prev, newFs]);
      setCreateName('');
      setCreateDescription('');
      setCreateIcon('');
      setShowCreateModal(false);

      success('Feature set created', `"${newFs.name}" has been created successfully`);

      // Automatically open the new feature set
      handleOpenPanel(newFs);
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to create feature set', errorMsg);
    } finally {
      setIsCreating(false);
    }
  };

  const handleDelete = async (id: string) => {
    // Confirmation handled by caller if needed, but we do it here too just in case called directly
    try {
      const deletedSet = featureSets.find((fs) => fs.id === id);
      await deleteFeatureSet(id);
      setFeatureSets((prev) => prev.filter((fs) => fs.id !== id));
      if (selectedFeatureSet?.id === id) {
        setSelectedFeatureSet(null);
      }

      success('Feature set deleted', `"${deletedSet?.name || 'Feature set'}" has been deleted`);
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to delete feature set', errorMsg);
    }
  };

  const handleOpenPanel = async (fs: FeatureSet) => {
    try {
      const fullFs = await getFeatureSetWithMembers(fs.id);
      if (fullFs) {
        setSelectedFeatureSet(fullFs);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handlePanelClose = () => {
    setSelectedFeatureSet(null);
    loadData(viewSpace?.id); // Refresh list to get updated member counts etc.
  };

  // Filter and sort feature sets (backend already filters server-all for disabled servers)
  const filteredSets = featureSets
    .filter((fs) => {
      // Hide implicit custom sets
      if (fs.name.endsWith(' - Custom')) return false;

      // Apply search filter
      if (!searchQuery) return true;
      const query = searchQuery.toLowerCase();
      return (
        fs.name.toLowerCase().includes(query) ||
        fs.description?.toLowerCase().includes(query) ||
        fs.feature_set_type.toLowerCase().includes(query)
      );
    })
    .sort((a, b) => {
      // Starter FS first (pinned to top — operator usually wants the
      // auto-seeded one near the top so they can edit / delete it
      // first), then Custom sets alphabetically. The 'default' key is
      // kept so a stale row read pre-migration still sorts correctly.
      const order: Record<string, number> = {
        starter: 0,
        default: 0,
        custom: 1,
      };
      const aOrder = order[a.feature_set_type] ?? 1;
      const bOrder = order[b.feature_set_type] ?? 1;
      if (aOrder !== bOrder) return aOrder - bOrder;
      return a.name.localeCompare(b.name);
    });

  return (
    <>
      <ToastContainer
        toasts={toasts}
        onClose={(id) => toasts.find((t) => t.id === id)?.onClose(id)}
      />
      <div className="relative flex h-full flex-col" data-testid="featuresets-page">
        {/* Header */}
        <div className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
          <div className="mx-auto max-w-[2000px]">
            <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
              <div className="min-w-0 flex-1">
                <div className="mb-2 flex flex-wrap items-center gap-3">
                  <h1 className="text-3xl font-bold tracking-tight">FeatureSets</h1>
                  {viewSpace && (
                    <span className="whitespace-nowrap rounded-full border border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] px-2 py-0.5 text-xs">
                      {viewSpace.icon || '📁'} {viewSpace.name}
                    </span>
                  )}
                </div>
                <p className="max-w-2xl text-base text-[rgb(var(--muted))]">
                  Curated bundles of tools, prompts, and resources — grant them to apps or map them
                  to folders in Workspaces
                </p>
              </div>
              <div className="flex flex-shrink-0 gap-3">
                <Button
                  variant="ghost"
                  size="md"
                  onClick={() => loadData(viewSpace?.id)}
                  disabled={isLoading}
                >
                  <RefreshCw className={`mr-2 h-4 w-4 ${isLoading ? 'animate-spin' : ''}`} />
                  Refresh
                </Button>
                <Button variant="primary" size="md" onClick={() => setShowCreateModal(true)}>
                  <Plus className="mr-2 h-4 w-4" />
                  Create Feature Set
                </Button>
              </div>
            </div>

            {/* Search Bar */}
            <div className="relative max-w-3xl">
              <Search className="absolute left-4 top-1/2 h-5 w-5 -translate-y-1/2 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder="Search feature sets..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="focus:ring-primary-500 focus:border-primary-500 w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] py-3 pl-12 pr-4 text-base transition-all focus:outline-none focus:ring-2"
              />
            </div>
          </div>
        </div>

        {/* Feature-set model explainer */}
        <div className="flex-shrink-0 px-8 pt-6">
          <div className="mx-auto flex max-w-[2000px] items-start gap-3 rounded-xl border border-emerald-200/70 bg-gradient-to-r from-emerald-50/60 to-transparent p-4 dark:border-emerald-800/40 dark:from-emerald-900/15">
            <div className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-emerald-500 to-green-500 text-white shadow-[0_4px_10px_-2px_rgb(16_185_129/0.45)]">
              <Zap className="h-4 w-4 fill-current" />
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold text-[rgb(var(--foreground))]">
                FeatureSets are bound to{' '}
                <span className="text-emerald-600 dark:text-emerald-400">workspace roots</span>
              </p>
              <p className="mt-0.5 text-xs leading-relaxed text-[rgb(var(--muted))]">
                Each Space gets one auto-created Default set. Routing is decided per reported folder
                via <span className="font-medium">Workspaces</span> — sessions whose root isn&apos;t
                bound fall back to the default Space&apos;s Default set.
              </p>
            </div>
          </div>
        </div>

        {/* Error */}
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
            ) : filteredSets.length === 0 ? (
              <Card className="mx-auto max-w-2xl">
                <CardContent className="flex flex-col items-center justify-center py-16">
                  <Package className="mb-4 h-16 w-16 text-[rgb(var(--muted))]" />
                  <h3 className="mb-2 text-lg font-medium">
                    {searchQuery ? 'No feature sets match your search' : 'No feature sets created'}
                  </h3>
                  <p className="mb-6 max-w-md text-center text-sm text-[rgb(var(--muted))]">
                    {searchQuery
                      ? 'Try adjusting your search terms'
                      : 'Create a feature set to group tools and resources together for easy access control.'}
                  </p>
                  {!searchQuery && (
                    <Button variant="primary" onClick={() => setShowCreateModal(true)}>
                      <Plus className="mr-2 h-4 w-4" />
                      Create Feature Set
                    </Button>
                  )}
                </CardContent>
              </Card>
            ) : (
              <div className="auto-fill-cards grid gap-5">
                {filteredSets.map((fs) => {
                  const isSelected = selectedFeatureSet?.id === fs.id;
                  const isBuiltin = fs.is_builtin;
                  const isStarter = isStarterFeatureSet(fs);

                  return (
                    <Card
                      key={fs.id}
                      className={`relative cursor-pointer overflow-hidden transition-all duration-300 hover:scale-[1.01] hover:shadow-lg ${
                        isSelected ? 'ring-primary-500 shadow-lg ring-2' : ''
                      } ${
                        isStarter
                          ? 'bg-gradient-to-br from-emerald-50/40 via-transparent to-transparent ring-1 ring-emerald-300 dark:from-emerald-900/15 dark:ring-emerald-700/60'
                          : ''
                      }`}
                      onClick={() => handleOpenPanel(fs)}
                      data-testid={`featureset-card-${fs.id}`}
                    >
                      {isStarter && (
                        <div
                          className="absolute right-3 top-3 flex items-center gap-1.5 rounded-full bg-gradient-to-r from-emerald-500 to-green-500 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-white shadow-[0_4px_12px_-2px_rgb(16_185_129/0.5)]"
                          title="Auto-seeded with this Space. Edit, rename, or delete freely — no special routing role; bindings and per-client grants pick FeatureSets explicitly."
                          data-testid={`featureset-starter-badge-${fs.id}`}
                        >
                          <CheckCircle2 className="h-3 w-3" />
                          Starter
                        </div>
                      )}

                      <CardContent className="p-6">
                        <div className="mb-5 flex items-start gap-4">
                          <div className="flex h-16 w-16 flex-shrink-0 items-center justify-center rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
                            {getFeatureSetIcon(fs)}
                          </div>
                          <div className="min-w-0 flex-1 pr-16">
                            <h3 className="mb-1.5 truncate text-lg font-semibold">{fs.name}</h3>
                            <span
                              className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${
                                isBuiltin
                                  ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300'
                                  : 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300'
                              }`}
                            >
                              {getFeatureSetTypeName(fs.feature_set_type)}
                            </span>
                          </div>
                        </div>

                        <p className="mb-4 line-clamp-2 h-10 text-sm text-[rgb(var(--muted))]">
                          {fs.description || 'No description provided.'}
                        </p>

                        <div className="flex items-center justify-between gap-3 border-t border-[rgb(var(--border-subtle))] pt-4 text-xs text-[rgb(var(--muted))]">
                          <span>{fs.members?.length || 0} members</span>
                          <span className="hover:text-primary-500 hidden flex-shrink-0 items-center gap-1 transition-colors md:flex">
                            Configure <Settings className="h-3 w-3" />
                          </span>
                        </div>
                      </CardContent>
                    </Card>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* Overlay backdrop when panel is open */}
        {selectedFeatureSet && (
          <div
            data-testid="featureset-panel-overlay"
            className="animate-in fade-in fixed inset-0 z-40 bg-black/20 backdrop-blur-[2px] duration-200"
            onClick={() => setSelectedFeatureSet(null)}
          />
        )}

        {/* Slide-out Panel */}
        {selectedFeatureSet && viewSpace && (
          <FeatureSetPanel
            featureSet={selectedFeatureSet}
            spaceId={viewSpace.id}
            onClose={handlePanelClose}
            onDelete={handleDelete}
            onUpdate={() => loadData(viewSpace.id)}
          />
        )}

        {/* Create Modal */}
        {showCreateModal && (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
            <Card className="animate-in fade-in zoom-in-95 mx-4 w-full max-w-md duration-200">
              <CardHeader>
                <CardTitle className="flex items-center justify-between">
                  <span className="flex items-center gap-2">
                    <Plus className="h-5 w-5" />
                    Create Feature Set
                  </span>
                  <button
                    onClick={() => setShowCreateModal(false)}
                    className="rounded p-1 hover:bg-[rgb(var(--surface-hover))]"
                  >
                    <X className="h-4 w-4" />
                  </button>
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div>
                  <label className="mb-1 block text-sm font-medium">Name *</label>
                  <input
                    type="text"
                    value={createName}
                    onChange={(e) => setCreateName(e.target.value)}
                    placeholder="e.g., GitHub Read Only"
                    className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 focus:outline-none focus:ring-2"
                    autoFocus
                  />
                </div>

                <div>
                  <label className="mb-1 block text-sm font-medium">Description</label>
                  <input
                    type="text"
                    value={createDescription}
                    onChange={(e) => setCreateDescription(e.target.value)}
                    placeholder="What this feature set allows..."
                    className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 focus:outline-none focus:ring-2"
                  />
                </div>

                <div>
                  <label className="mb-1 block text-sm font-medium">Icon (emoji)</label>
                  <input
                    type="text"
                    value={createIcon}
                    onChange={(e) => setCreateIcon(e.target.value)}
                    placeholder="🔧"
                    className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 focus:outline-none focus:ring-2"
                    maxLength={2}
                  />
                </div>

                <div className="flex gap-3 pt-2">
                  <Button variant="ghost" onClick={() => setShowCreateModal(false)}>
                    Cancel
                  </Button>
                  <Button
                    variant="primary"
                    onClick={handleCreate}
                    disabled={isCreating || !createName.trim()}
                  >
                    {isCreating ? <Loader2 className="h-4 w-4 animate-spin" /> : 'Create'}
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

export default FeatureSetsPage;
