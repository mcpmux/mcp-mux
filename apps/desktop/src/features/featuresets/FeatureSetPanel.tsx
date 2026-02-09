import { useState, useEffect } from 'react';
import {
  X,
  Loader2,
  Search,
  Server,
  Wrench,
  MessageSquare,
  FileText,
  Package,
  ChevronDown,
  ChevronRight,
  ToggleLeft,
  ToggleRight,
  Settings,
  Trash2,
  Check,
  Globe,
  Star,
  Shield,
  Save,
} from 'lucide-react';
import { Button, useToast, ToastContainer } from '@mcpmux/ui';
import type { FeatureSet, AddMemberInput } from '@/lib/api/featureSets';
import { setFeatureSetMembers } from '@/lib/api/featureSets';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { listServerFeatures } from '@/lib/api/serverFeatures';

interface FeatureSetPanelProps {
  featureSet: FeatureSet;
  spaceId: string;
  onClose: () => void;
  onDelete?: (id: string) => void;
  onUpdate?: () => void;
}

interface ServerGroup {
  serverId: string;
  features: ServerFeature[];
  isExpanded: boolean;
}

export function FeatureSetPanel({ featureSet, spaceId, onClose, onDelete, onUpdate }: FeatureSetPanelProps) {
  const [allFeatures, setAllFeatures] = useState<ServerFeature[]>([]);
  const [selectedFeatureIds, setSelectedFeatureIds] = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedServers, setExpandedServers] = useState<Set<string>>(new Set());
  const { toasts, success, error: showError, dismiss } = useToast();

  // Collapsible sections - only one expanded at a time, features by default
  const [expandedSections, setExpandedSections] = useState({
    settings: false,
    features: true,
  });

  // Determine if this is a configurable feature set
  const isConfigurable = featureSet.feature_set_type === 'default' || featureSet.feature_set_type === 'custom';
  const isDefault = featureSet.feature_set_type === 'default';
  const isCustom = featureSet.feature_set_type === 'custom';
  const isAll = featureSet.feature_set_type === 'all';
  const isServerAll = featureSet.feature_set_type === 'server-all';
  
  // For special feature sets, compute actual member count
  const getActualMemberCount = () => {
    if (isAll) {
      // "All Features" includes everything
      return allFeatures.length;
    }
    if (isServerAll && featureSet.server_id) {
      // "Server All" - use server_id from feature set
      return allFeatures.filter(f => f.server_id === featureSet.server_id).length;
    }
    // For configurable sets, use selectedFeatureIds
    return selectedFeatureIds.size;
  };
  
  // Check if a feature should be shown as selected
  const isFeatureSelected = (featureId: string, feature: ServerFeature) => {
    if (isAll) {
      // All features are selected
      return true;
    }
    if (isServerAll && featureSet.server_id) {
      // Only features from the target server
      return feature.server_id === featureSet.server_id;
    }
    // For configurable sets, check selectedFeatureIds
    return selectedFeatureIds.has(featureId);
  };

  useEffect(() => {
    const loadFeatures = async () => {
      setIsLoading(true);
      try {
        const features = await listServerFeatures(spaceId);
        setAllFeatures(features);
        
        // Initialize selected features from current members
        const currentIds = new Set<string>();
        
        // For special feature sets, compute selection dynamically
        if (featureSet.feature_set_type === 'all') {
          // All features are selected
          features.forEach(f => currentIds.add(f.id));
        } else if (featureSet.feature_set_type === 'server-all' && featureSet.server_id) {
          // All features from this server are selected
          features.forEach(f => {
            if (f.server_id === featureSet.server_id) {
              currentIds.add(f.id);
            }
          });
        } else {
          // For configurable sets (default/custom), use members array
          featureSet.members?.forEach((m) => {
            if (m.member_type === 'feature' && m.mode === 'include') {
              currentIds.add(m.member_id);
            }
          });
        }
        
        setSelectedFeatureIds(currentIds);
        
        // Start with all servers collapsed
        setExpandedServers(new Set());
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setIsLoading(false);
      }
    };
    
    loadFeatures();
  }, [spaceId, featureSet]);

  // Group features by server
  const serverGroups: ServerGroup[] = allFeatures.reduce((acc, feature) => {
    const group = acc.find((g) => g.serverId === feature.server_id);
    if (group) {
      group.features.push(feature);
    } else {
      acc.push({ 
        serverId: feature.server_id, 
        features: [feature],
        isExpanded: expandedServers.has(feature.server_id)
      });
    }
    return acc;
  }, [] as ServerGroup[]);

  // Filter by search
  const filteredGroups = serverGroups
    .map((group) => ({
      ...group,
      features: group.features.filter((f) =>
        f.feature_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
        f.display_name?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        f.description?.toLowerCase().includes(searchQuery.toLowerCase())
      ),
    }))
    .filter((group) => group.features.length > 0);

  const toggleFeature = (featureId: string) => {
    if (!isConfigurable) return;
    setSelectedFeatureIds((prev) => {
      const next = new Set(prev);
      if (next.has(featureId)) {
        next.delete(featureId);
      } else {
        next.add(featureId);
      }
      return next;
    });
  };

  const toggleServer = (serverId: string) => {
    setExpandedServers((prev) => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
      }
      return next;
    });
  };

  const toggleAllInServer = (serverId: string) => {
    if (!isConfigurable) return;
    const serverFeatures = allFeatures.filter((f) => f.server_id === serverId);
    const allSelected = serverFeatures.every((f) => selectedFeatureIds.has(f.id));
    
    setSelectedFeatureIds((prev) => {
      const next = new Set(prev);
      serverFeatures.forEach((f) => {
        if (allSelected) {
          next.delete(f.id);
        } else {
          next.add(f.id);
        }
      });
      return next;
    });
  };

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);
    try {
      // Update members
      const members: AddMemberInput[] = Array.from(selectedFeatureIds).map((id) => ({
        member_type: 'feature' as const,
        member_id: id,
        mode: 'include' as const,
      }));
      
      await setFeatureSetMembers(featureSet.id, members);

      success('Changes saved', `"${featureSet.name}" has been updated with ${members.length} feature${members.length !== 1 ? 's' : ''}`);
      onUpdate?.();
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to save changes', errorMsg);
    } finally {
      setIsSaving(false);
    }
  };

  const getFeatureIcon = (type: string) => {
    switch (type) {
      case 'tool':
        return <Wrench className="h-4 w-4 text-purple-500" />;
      case 'prompt':
        return <MessageSquare className="h-4 w-4 text-blue-500" />;
      case 'resource':
        return <FileText className="h-4 w-4 text-green-500" />;
      default:
        return <Package className="h-4 w-4 text-gray-500" />;
    }
  };

  const getTypeColor = (type: string) => {
    switch (type) {
      case 'tool':
        return 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300';
      case 'prompt':
        return 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300';
      case 'resource':
        return 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300';
      default:
        return 'bg-gray-100 dark:bg-gray-800 text-gray-700 dark:text-gray-300';
    }
  };

  const getFeatureSetIcon = () => {
    if (featureSet.icon) return <span className="text-xl">{featureSet.icon}</span>;
    switch (featureSet.feature_set_type) {
      case 'all': return <Globe className="h-6 w-6 text-green-500" />;
      case 'default': return <Star className="h-6 w-6 text-yellow-500" />;
      case 'server-all': return <Server className="h-6 w-6 text-blue-500" />;
      case 'custom': default: return <Package className="h-6 w-6 text-purple-500" />;
    }
  };

  const toggleSection = (section: keyof typeof expandedSections) => {
    setExpandedSections(prev => {
      // Accordion behavior - close others when opening a section
      if (!prev[section]) {
        return { settings: false, features: false, [section]: true };
      }
      // Allow closing the current section
      return { ...prev, [section]: false };
    });
  };

  return (
    <div className="fixed right-0 top-0 bottom-0 w-full max-w-[45%] min-w-[600px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {/* Panel Header */}
      <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
        <div className="flex items-start justify-between mb-3">
          <div className="flex items-center gap-3 flex-1 min-w-0">
            <div className="w-10 h-10 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg flex-shrink-0 border border-[rgb(var(--border))]">
              {getFeatureSetIcon()}
            </div>
            <div className="flex-1 min-w-0">
              <h2 className="text-lg font-bold truncate flex items-center gap-2">
                {featureSet.name}
              </h2>
              <div className="flex items-center gap-2 mt-0.5">
                <span className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium border ${
                  isDefault 
                    ? 'bg-yellow-50 dark:bg-yellow-900/20 text-yellow-700 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800'
                    : isCustom
                      ? 'bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-400 border-purple-200 dark:border-purple-800'
                      : 'bg-gray-50 dark:bg-gray-900/20 text-gray-700 dark:text-gray-400 border-gray-200 dark:border-gray-800'
                }`}>
                  {featureSet.feature_set_type.toUpperCase()}
                </span>
                <span className="text-xs text-[rgb(var(--muted))] truncate">
                  ID: {featureSet.id}
                </span>
              </div>
            </div>
          </div>
          <button
            data-testid="featureset-panel-close"
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      {/* Scrollable Content */}
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 space-y-5">
          {/* Error */}
          {error && (
            <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-600 dark:text-red-400">
              {error}
            </div>
          )}

          {/* Info Section (Read-only for non-custom/default) */}
          <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden">
            <button
              onClick={() => toggleSection('settings')}
              className={`w-full flex items-center justify-between p-4 transition-all ${
                expandedSections.settings 
                  ? 'bg-gradient-to-r from-primary-50 to-primary-100/50 dark:from-primary-900/10 dark:to-primary-800/10' 
                  : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              <div className="flex items-center gap-3">
                <div className={`p-2 rounded-lg ${
                  expandedSections.settings
                    ? 'bg-gray-500 text-white'
                    : 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400'
                }`}>
                  <Settings className="h-5 w-5" />
                </div>
                <span className="font-semibold text-base">General Information</span>
              </div>
              {expandedSections.settings ? (
                <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
              ) : (
                <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
              )}
            </button>
            
            {expandedSections.settings && (
              <div className="p-4 space-y-4 border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                <div>
                  <label className="block text-xs font-medium mb-1.5 text-[rgb(var(--muted))]">
                    Description
                  </label>
                  <p className="text-sm">
                    {featureSet.description || 'No description provided.'}
                  </p>
                </div>
                
                {isDefault && (
                  <div className="p-3 bg-yellow-50 dark:bg-yellow-900/10 border border-yellow-200 dark:border-yellow-800 rounded-lg">
                    <div className="flex gap-2">
                      <Star className="h-4 w-4 text-yellow-500 flex-shrink-0 mt-0.5" />
                      <div className="text-xs text-yellow-800 dark:text-yellow-200">
                        <strong>Default Feature Set:</strong> Features selected here are automatically granted to all clients in this workspace.
                      </div>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Feature Selection Section */}
          <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden">
            <button
              onClick={() => toggleSection('features')}
              className={`w-full flex items-center justify-between p-4 transition-all ${
                expandedSections.features
                  ? 'bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20' 
                  : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              <div className="flex items-center gap-3 flex-1">
                <div className={`p-2 rounded-lg ${
                  expandedSections.features
                    ? 'bg-blue-500 text-white'
                    : 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                }`}>
                  <Shield className="h-5 w-5" />
                </div>
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-semibold text-base">Included Features</span>
                    {/* Show count badge only for configurable feature sets */}
                    {isConfigurable && (
                      <span className={`text-xs px-2.5 py-1 rounded-full font-bold ${
                        getActualMemberCount() > 0
                          ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border border-green-300 dark:border-green-700'
                          : 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400 border border-gray-300 dark:border-gray-700'
                      }`}>
                        {getActualMemberCount()} / {allFeatures.length} selected
                      </span>
                    )}
                  </div>
                  {/* Progress Bar */}
                  <div className="h-1.5 bg-gray-200 dark:bg-gray-800 rounded-full overflow-hidden">
                    <div 
                      className={`h-full transition-all duration-300 ${
                        getActualMemberCount() === 0 
                          ? 'bg-gray-400 dark:bg-gray-600' 
                          : 'bg-gradient-to-r from-green-500 to-blue-500'
                      }`}
                      style={{ 
                        width: `${allFeatures.length > 0 ? (getActualMemberCount() / allFeatures.length * 100) : 0}%` 
                      }}
                    />
                  </div>
                </div>
              </div>
              {expandedSections.features ? (
                <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
              ) : (
                <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
              )}
            </button>

            {expandedSections.features && (
              <div className="border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))] flex flex-col h-[500px]">
                {/* Search Bar inside panel */}
                <div className="p-3 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface))]">
                  <div className="relative">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))]" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder="Search features..."
                      className="w-full pl-9 pr-3 py-2 text-sm rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                    />
                  </div>
                </div>

                <div className="flex-1 overflow-y-auto">
                  {isLoading ? (
                    <div className="flex items-center justify-center h-full">
                      <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
                    </div>
                  ) : filteredGroups.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-[rgb(var(--muted))] p-4 text-center">
                      <Package className="h-8 w-8 mb-2 opacity-50" />
                      <p className="text-sm">No features found matching your search</p>
                    </div>
                  ) : (
                    <div className="divide-y divide-[rgb(var(--border))]">
                      {filteredGroups.map((group) => {
                        // For special sets, use isFeatureSelected logic
                        const selectedCount = group.features.filter((f) =>
                          isFeatureSelected(f.id, f)
                        ).length;
                        const allSelected = selectedCount === group.features.length;
                        const someSelected = selectedCount > 0 && selectedCount < group.features.length;
                        const isExpanded = group.isExpanded;
                        
                        return (
                          <div key={group.serverId} className="bg-[rgb(var(--surface))]">
                            <div 
                              className="flex items-center justify-between px-4 py-3 hover:bg-[rgb(var(--surface-hover))] cursor-pointer transition-colors"
                              onClick={() => toggleServer(group.serverId)}
                            >
                              <div className="flex items-center gap-3 flex-1 min-w-0">
                                {isExpanded ? (
                                  <ChevronDown className="h-4 w-4 text-[rgb(var(--muted))] flex-shrink-0" />
                                ) : (
                                  <ChevronRight className="h-4 w-4 text-[rgb(var(--muted))] flex-shrink-0" />
                                )}
                                <Server className="h-4 w-4 text-blue-500 flex-shrink-0" />
                                <div className="flex-1 min-w-0">
                                  <div className="flex items-center gap-2 mb-1">
                                    <span className="font-medium text-sm truncate">{group.serverId}</span>
                                    {/* Show count badge only for configurable feature sets */}
                                    {isConfigurable && (
                                      <span className={`text-xs px-2 py-0.5 rounded-full font-bold flex-shrink-0 ${
                                        selectedCount === 0
                                          ? 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400'
                                          : allSelected
                                            ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border border-green-300 dark:border-green-700'
                                            : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 border border-amber-300 dark:border-amber-700'
                                      }`}>
                                        {selectedCount}/{group.features.length}
                                      </span>
                                    )}
                                  </div>
                                  {/* Progress Bar for Server */}
                                  <div className="h-1 bg-gray-200 dark:bg-gray-800 rounded-full overflow-hidden">
                                    <div 
                                      className={`h-full transition-all duration-300 ${
                                        selectedCount === 0 
                                          ? 'bg-gray-400 dark:bg-gray-600' 
                                          : allSelected
                                            ? 'bg-green-500'
                                            : 'bg-gradient-to-r from-amber-500 to-green-500'
                                      }`}
                                      style={{ 
                                        width: `${(selectedCount / group.features.length * 100)}%` 
                                      }}
                                    />
                                  </div>
                                </div>
                              </div>
                              
                              {isConfigurable && (
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    toggleAllInServer(group.serverId);
                                  }}
                                  className={`p-1.5 rounded-md transition-colors hover:bg-[rgb(var(--background))] flex-shrink-0`}
                                  title={allSelected ? "Disable All" : "Enable All"}
                                >
                                  {allSelected ? (
                                    <ToggleRight className="h-5 w-5 text-primary-500" />
                                  ) : someSelected ? (
                                    <ToggleLeft className="h-5 w-5 text-amber-500" />
                                  ) : (
                                    <ToggleLeft className="h-5 w-5 text-[rgb(var(--muted))]" />
                                  )}
                                </button>
                              )}
                            </div>
                            
                            {isExpanded && (
                              <div className="bg-[rgb(var(--background))] border-t border-[rgb(var(--border))]">
                                {group.features.map((feature) => {
                                  const isSelected = isFeatureSelected(feature.id, feature);
                                  
                                  return (
                                    <button
                                      key={feature.id}
                                      onClick={() => toggleFeature(feature.id)}
                                      disabled={!isConfigurable}
                                      className={`w-full flex items-center gap-3 px-4 py-2.5 pl-12 text-left border-b border-[rgb(var(--border))] last:border-b-0 transition-colors
                                        ${isConfigurable ? 'hover:bg-[rgb(var(--surface-hover))]' : 'cursor-default'}
                                        ${isSelected ? 'bg-primary-50 dark:bg-primary-900/10' : ''}`}
                                    >
                                      <div className={`flex-shrink-0 w-4 h-4 rounded border flex items-center justify-center transition-colors ${
                                        isSelected 
                                          ? 'bg-primary-500 border-primary-500' 
                                          : 'border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--surface))]'
                                      }`}>
                                        {isSelected && <Check className="h-3 w-3 text-white" />}
                                      </div>
                                      
                                      {getFeatureIcon(feature.feature_type)}
                                      
                                      <div className="flex-1 min-w-0">
                                        <div className="flex items-center gap-2">
                                          <span className="font-medium text-sm truncate">
                                            {feature.display_name || feature.feature_name}
                                          </span>
                                          <span className={`text-[10px] px-1.5 py-0.5 rounded ${getTypeColor(feature.feature_type)}`}>
                                            {feature.feature_type}
                                          </span>
                                        </div>
                                        {feature.description && (
                                          <p className="text-xs text-[rgb(var(--muted))] mt-0.5 line-clamp-1">
                                            {feature.description}
                                          </p>
                                        )}
                                      </div>
                                    </button>
                                  );
                                })}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Footer Actions */}
      <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] flex items-center gap-3">
        {isCustom && onDelete && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              if (confirm('Delete this feature set?')) {
                onDelete(featureSet.id);
              }
            }}
            className="text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 mr-auto"
          >
            <Trash2 className="h-4 w-4 mr-2" />
            Delete
          </Button>
        )}
        
        {isConfigurable && (
          <Button
            onClick={handleSave}
            disabled={isSaving}
            className="w-full flex-1"
          >
            {isSaving ? (
              <><Loader2 className="h-4 w-4 mr-2 animate-spin" /> Saving...</>
            ) : (
              <><Save className="h-4 w-4 mr-2" /> Save Changes</>
            )}
          </Button>
        )}
      </div>
    </div>
  );
}
