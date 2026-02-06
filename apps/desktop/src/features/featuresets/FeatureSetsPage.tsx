import { useState, useEffect, useCallback } from 'react';
import {
  Plus,
  Loader2,
  Server,
  Package,
  Settings,
  X,
  RefreshCw,
  Globe,
  Star,
  Search,
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
import type { FeatureSet, CreateFeatureSetInput } from '@/lib/api/featureSets';
import {
  listFeatureSetsBySpace,
  createFeatureSet,
  deleteFeatureSet,
  getFeatureSetWithMembers,
} from '@/lib/api/featureSets';
import { useViewSpace } from '@/stores';
import { FeatureSetPanel } from './FeatureSetPanel';

// Get icon for feature set type
const getFeatureSetIcon = (fs: FeatureSet) => {
  if (fs.icon) return <span className="text-xl">{fs.icon}</span>;
  
  switch (fs.feature_set_type) {
    case 'all':
      return <Globe className="h-8 w-8 text-green-500" />;
    case 'default':
      return <Star className="h-8 w-8 text-yellow-500" />;
    case 'server-all':
      return <Server className="h-8 w-8 text-blue-500" />;
    case 'custom':
    default:
      return <Package className="h-8 w-8 text-purple-500" />;
  }
};

// Get display name for feature set type
const getFeatureSetTypeName = (type: string) => {
  switch (type) {
    case 'all':
      return 'All Features';
    case 'default':
      return 'Default';
    case 'server-all':
      return 'Server All';
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
      
      // Backend filters out server-all feature sets for disabled servers
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
      const deletedSet = featureSets.find(fs => fs.id === id);
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

  // Filter feature sets (backend already filters server-all for disabled servers)
  const filteredSets = featureSets.filter(fs => {
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
  });

  return (
    <>
      <ToastContainer toasts={toasts} onClose={(id) => toasts.find(t => t.id === id)?.onClose(id)} />
      <div className="h-full flex flex-col relative" data-testid="featuresets-page">
      {/* Header */}
      <div className="flex-shrink-0 p-8 border-b border-[rgb(var(--border-subtle))]">
        <div className="max-w-[2000px] mx-auto">
          <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-4 mb-6">
            <div className="flex-1 min-w-0">
              <div className="flex flex-wrap items-center gap-3 mb-2">
                <h1 className="text-3xl font-bold">Feature Sets</h1>
                {viewSpace && (
                  <span className="px-2 py-0.5 rounded-full bg-[rgb(var(--surface-elevated))] text-xs border border-[rgb(var(--border))] whitespace-nowrap">
                    {viewSpace.icon || '📁'} {viewSpace.name}
                  </span>
                )}
              </div>
              <p className="text-base text-[rgb(var(--muted))]">
                Manage reusable collections of features, prompts, and resources
              </p>
            </div>
            <div className="flex gap-3 flex-shrink-0">
              <Button 
                variant="ghost" 
                size="md" 
                onClick={() => loadData(viewSpace?.id)}
                disabled={isLoading}
              >
                <RefreshCw className={`h-4 w-4 mr-2 ${isLoading ? 'animate-spin' : ''}`} />
                Refresh
              </Button>
              <Button variant="primary" size="md" onClick={() => setShowCreateModal(true)}>
                <Plus className="h-4 w-4 mr-2" />
                Create Feature Set
              </Button>
            </div>
          </div>

          {/* Search Bar */}
          <div className="relative max-w-3xl">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
            <input
              type="text"
              placeholder="Search feature sets..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full pl-12 pr-4 py-3 text-base bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-xl focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 transition-all"
            />
          </div>
        </div>
      </div>

      {/* Error */}
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
          ) : filteredSets.length === 0 ? (
            <Card className="max-w-2xl mx-auto">
              <CardContent className="flex flex-col items-center justify-center py-16">
                <Package className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
                <h3 className="text-lg font-medium mb-2">
                  {searchQuery ? 'No feature sets match your search' : 'No feature sets created'}
                </h3>
                <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
                  {searchQuery 
                    ? 'Try adjusting your search terms' 
                    : 'Create a feature set to group tools and resources together for easy access control.'
                  }
                </p>
                {!searchQuery && (
                  <Button variant="primary" onClick={() => setShowCreateModal(true)}>
                    <Plus className="h-4 w-4 mr-2" />
                    Create Feature Set
                  </Button>
                )}
              </CardContent>
            </Card>
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filteredSets.map((fs) => {
                const isSelected = selectedFeatureSet?.id === fs.id;
                const isBuiltin = fs.is_builtin;
                
                return (
                  <Card 
                    key={fs.id}
                    className={`cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] ${
                      isSelected ? 'ring-2 ring-primary-500 shadow-lg' : ''
                    }`}
                    onClick={() => handleOpenPanel(fs)}
                    data-testid={`featureset-card-${fs.id}`}
                  >
                    <CardContent className="p-6">
                      {/* Header */}
                      <div className="flex items-start gap-4 mb-5">
                        <div className="w-16 h-16 flex items-center justify-center bg-[rgb(var(--surface))] rounded-xl flex-shrink-0 border border-[rgb(var(--border-subtle))]">
                          {getFeatureSetIcon(fs)}
                        </div>
                        <div className="flex-1 min-w-0">
                          <h3 className="font-semibold text-lg truncate mb-1.5 flex items-center gap-2">
                            {fs.name}
                          </h3>
                          <span className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
                            isBuiltin 
                              ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300' 
                              : 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
                          }`}>
                            {getFeatureSetTypeName(fs.feature_set_type)}
                          </span>
                        </div>
                      </div>

                      {/* Description */}
                      <p className="text-sm text-[rgb(var(--muted))] line-clamp-2 mb-4 h-10">
                        {fs.description || 'No description provided.'}
                      </p>

                      {/* Footer Info */}
                      <div className="flex items-center justify-between text-xs text-[rgb(var(--muted))] border-t border-[rgb(var(--border-subtle))] pt-4">
                        <div className="flex items-center gap-1.5">
                          {fs.feature_set_type === 'server-all' ? (
                            <span className="truncate max-w-[150px]">{fs.server_id}</span>
                          ) : fs.feature_set_type === 'all' ? (
                            <span className="italic">All features</span>
                          ) : (
                            <span>{fs.members?.length || 0} members</span>
                          )}
                        </div>
                        {isBuiltin && fs.feature_set_type !== 'default' ? (
                          <span className="italic">Auto-managed</span>
                        ) : (
                          <span className="flex items-center gap-1 hover:text-primary-500 transition-colors">
                            Configure <Settings className="h-3 w-3" />
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

      {/* Overlay backdrop when panel is open */}
      {selectedFeatureSet && (
        <div 
          data-testid="featureset-panel-overlay"
          className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
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
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <Card className="w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-200">
            <CardHeader>
              <CardTitle className="flex items-center justify-between">
                <span className="flex items-center gap-2">
                  <Plus className="h-5 w-5" />
                  Create Feature Set
                </span>
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="p-1 rounded hover:bg-[rgb(var(--surface-hover))]"
                >
                  <X className="h-4 w-4" />
                </button>
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div>
                <label className="block text-sm font-medium mb-1">Name *</label>
                <input
                  type="text"
                  value={createName}
                  onChange={(e) => setCreateName(e.target.value)}
                  placeholder="e.g., GitHub Read Only"
                  className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                  autoFocus
                />
              </div>
              
              <div>
                <label className="block text-sm font-medium mb-1">Description</label>
                <input
                  type="text"
                  value={createDescription}
                  onChange={(e) => setCreateDescription(e.target.value)}
                  placeholder="What this feature set allows..."
                  className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                />
              </div>
              
              <div>
                <label className="block text-sm font-medium mb-1">Icon (emoji)</label>
                <input
                  type="text"
                  value={createIcon}
                  onChange={(e) => setCreateIcon(e.target.value)}
                  placeholder="🔧"
                  className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] focus:outline-none focus:ring-2 focus:ring-primary-500"
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
