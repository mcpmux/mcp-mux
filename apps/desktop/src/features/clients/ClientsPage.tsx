import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import { resolveKnownClientKey } from '@/lib/clientIcons';
import {
  Laptop,
  Loader2,
  Lock,
  Unlock,
  HelpCircle,
  Wifi,
  WifiOff,
  RefreshCw,
  Settings,
  Trash2,
  X,
  Check,
  ChevronDown,
  ChevronRight,
  Shield,
  Layers,
  Search,
  AlertCircle,
  Zap,
} from 'lucide-react';
import {
  Card,
  CardContent,
  Button,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import type { OAuthClient, UpdateClientRequest } from '@/lib/api/gateway';
import { listOAuthClients, updateOAuthClient, deleteOAuthClient } from '@/lib/api/gateway';
import type { Space } from '@/lib/api/spaces';
import { listSpaces } from '@/lib/api/spaces';
import { useViewSpace } from '@/stores';
import type { FeatureSet } from '@/lib/api/featureSets';
import { listFeatureSetsBySpace } from '@/lib/api/featureSets';
import { 
  getOAuthClientGrants, 
  grantOAuthClientFeatureSet, 
  revokeOAuthClientFeatureSet,
  getOAuthClientResolvedFeatures 
} from '@/lib/api/oauthClients';
import {
  addFeatureToSet,
  removeFeatureFromSet,
  getFeatureSetMembers,
  type FeatureSetMember
} from '@/lib/api/featureMembers';
import { listServerFeatures } from '@/lib/api/serverFeatures';
import { invoke } from '@tauri-apps/api/core';

// Connection mode options
const CONNECTION_MODES = [
  {
    value: 'follow_active',
    label: 'Follow Active Space',
    icon: Unlock,
    color: 'text-green-500',
    description: 'Automatically use your currently active space',
  },
  {
    value: 'locked',
    label: 'Locked to Space',
    icon: Lock,
    color: 'text-blue-500',
    description: 'Always use a specific space',
  },
  {
    value: 'ask_on_change',
    label: 'Ask on Change',
    icon: HelpCircle,
    color: 'text-orange-500',
    description: 'Prompt when switching spaces',
  },
];

// Bundled icons for well-known AI clients (resolved via icon key)
const CLIENT_ICON_ASSETS: Record<string, string> = {
  cursor: cursorIcon,
  vscode: vscodeIcon,
  claude: claudeIcon,
  windsurf: windsurfIcon,
};

// Client icon component — uses bundled icon for known clients, falls back to logo_uri, then emoji
function ClientIcon({ logo_uri, client_name }: { logo_uri?: string | null; client_name: string }) {
  const knownKey = resolveKnownClientKey(client_name);
  const iconUrl = (knownKey && CLIENT_ICON_ASSETS[knownKey]) || logo_uri;
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={client_name}
        className="w-full h-full object-contain rounded"
        onError={(e) => {
          e.currentTarget.style.display = 'none';
          e.currentTarget.parentElement!.append(document.createTextNode('🤖'));
        }}
      />
    );
  }
  return <span>🤖</span>;
}

export default function ClientsPage() {
  const [oauthClients, setOAuthClients] = useState<OAuthClient[]>([]);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshingOAuth, setIsRefreshingOAuth] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  
  // Panel state
  const [selectedClient, setSelectedClient] = useState<OAuthClient | null>(null);
  
  const { toasts, success, error: showError, info, dismiss } = useToast();
  
  // Edit state
  const [editAlias, setEditAlias] = useState('');
  const [editMode, setEditMode] = useState('follow_active');
  const [editLockedSpaceId, setEditLockedSpaceId] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  
  // Feature set grant state
  const viewSpace = useViewSpace();
  const [activeSpace, setActiveSpace] = useState<Space | null>(null);
  const [availableFeatureSets, setAvailableFeatureSets] = useState<FeatureSet[]>([]);
  const [grantedFeatureSetIds, setGrantedFeatureSetIds] = useState<string[]>([]);
  const [isLoadingGrants, setIsLoadingGrants] = useState(false);
  
  // Resolved features state
  const [resolvedFeatures, setResolvedFeatures] = useState<{
    tools: Array<{ name: string; description?: string; server_id: string }>;
    prompts: Array<{ name: string; description?: string; server_id: string }>;
    resources: Array<{ name: string; description?: string; server_id: string }>;
  } | null>(null);
  const [isLoadingResolvedFeatures, setIsLoadingResolvedFeatures] = useState(false);
  
  // Individual features management
  const [availableFeatures, setAvailableFeatures] = useState<Array<{
    id: string;
    feature_name: string;
    feature_type: string;
    description?: string;
    server_id: string;
  }>>([]);
  const [clientCustomFeatureSet, setClientCustomFeatureSet] = useState<FeatureSet | null>(null);
  const [individualFeatureMembers, setIndividualFeatureMembers] = useState<FeatureSetMember[]>([]);
  const [isLoadingFeatures, setIsLoadingFeatures] = useState(false);
  
  // Collapsible sections
  const [expandedSections, setExpandedSections] = useState({
    quickSettings: true,
    permissions: true,
    effectiveFeatures: false,
    advancedPermissions: false,
    clientInfo: false,
  });
  const [expandedServers, setExpandedServers] = useState<Set<string>>(new Set());
  const [expandedFeatureTypes, setExpandedFeatureTypes] = useState({
    tools: false,
    prompts: false,
    resources: false,
  });

  const toggleSection = (section: keyof typeof expandedSections) => {
    setExpandedSections(prev => {
      const isCurrentlyExpanded = prev[section];
      
      // If clicking on an already expanded section, just toggle it
      if (isCurrentlyExpanded) {
        return { ...prev, [section]: false };
      }
      
      // Otherwise, collapse all and expand the clicked one
      return {
        quickSettings: false,
        permissions: false,
        effectiveFeatures: false,
        advancedPermissions: false,
        clientInfo: false,
        [section]: true,
      };
    });
  };

  const loadData = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [oauthData, spacesData] = await Promise.all([
        listOAuthClients().catch(() => [] as OAuthClient[]),
        listSpaces().catch(() => [] as Space[]),
      ]);
      setOAuthClients(oauthData);
      setSpaces(spacesData);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const loadGrantsForClient = async (clientId: string) => {
    if (!activeSpace) return;
    
    setIsLoadingGrants(true);
    try {
      const [featureSets, grants] = await Promise.all([
        listFeatureSetsBySpace(activeSpace.id),
        getOAuthClientGrants(clientId, activeSpace.id),
      ]);
      setAvailableFeatureSets(featureSets);
      setGrantedFeatureSetIds(grants);
    } catch (e) {
      console.warn('Failed to load grants:', e);
    } finally {
      setIsLoadingGrants(false);
    }
  };

  const loadResolvedFeatures = async (clientId: string, client?: OAuthClient) => {
    const targetClient = client ?? selectedClient;
    if (!activeSpace || !targetClient) return;
    
    setIsLoadingResolvedFeatures(true);
    try {
      const resolveSpaceId = targetClient.connection_mode === 'locked' && targetClient.locked_space_id
        ? targetClient.locked_space_id
        : activeSpace.id;
        
      const resolved = await getOAuthClientResolvedFeatures(clientId, resolveSpaceId);
      setResolvedFeatures({
        tools: resolved.tools,
        prompts: resolved.prompts,
        resources: resolved.resources,
      });
    } catch (e) {
      console.warn('Failed to load resolved features:', e);
      setResolvedFeatures(null);
    } finally {
      setIsLoadingResolvedFeatures(false);
    }
  };

  const refreshOAuthClients = async () => {
    setIsRefreshingOAuth(true);
    try {
      const oauthData = await listOAuthClients();
      setOAuthClients(oauthData);
    } catch (e) {
      console.warn('Failed to refresh OAuth clients:', e);
    } finally {
      setIsRefreshingOAuth(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  useEffect(() => {
    setActiveSpace(viewSpace);
  }, [viewSpace?.id]);

  useEffect(() => {
    if (!selectedClient || !activeSpace) return;
    loadGrantsForClient(selectedClient.client_id);
    loadAvailableFeatures();
    loadClientCustomFeatureSet(selectedClient);
    loadResolvedFeatures(selectedClient.client_id);
  }, [activeSpace?.id, selectedClient?.client_id]);

  useEffect(() => {
    const unlistenDomain = listen<{ action: string; client_id: string; client_name?: string }>('client-changed', (event) => {
      console.log('Client changed (domain):', event.payload);
      refreshOAuthClients();
      
      // Show toast for reconnections (silent approval)
      if (event.payload.action === 'reconnected') {
        const name = event.payload.client_name || event.payload.client_id;
        info('Client connected', `${name} connected`);
      }
    });

    const unlistenOAuth = listen('oauth-client-changed', (event) => {
      console.log('OAuth client changed:', event.payload);
      refreshOAuthClients();
    });

    return () => {
      unlistenDomain.then(fn => fn());
      unlistenOAuth.then(fn => fn());
    };
  }, []);

  const openPanel = async (client: OAuthClient) => {
    setSelectedClient(client);
    setEditAlias(client.client_alias || '');
    setEditMode(client.connection_mode);
    setEditLockedSpaceId(client.locked_space_id || '');
    
    // Reset collapsible states
    setExpandedSections({
      quickSettings: true,
      permissions: true,
      effectiveFeatures: false,
      advancedPermissions: false,
      clientInfo: false,
    });
    setExpandedServers(new Set());
    setExpandedFeatureTypes({ tools: false, prompts: false, resources: false });
    
    await Promise.all([
      loadGrantsForClient(client.client_id),
      loadAvailableFeatures(),
    ]);
    
    await loadClientCustomFeatureSet(client);
    loadResolvedFeatures(client.client_id, client);
  };

  const toggleFeatureSetGrant = async (featureSetId: string) => {
    if (!selectedClient || !activeSpace) return;
    
    const featureSet = availableFeatureSets.find(fs => fs.id === featureSetId);
    const fsName = featureSet?.name || 'Feature set';
    
    try {
      if (grantedFeatureSetIds.includes(featureSetId)) {
        await revokeOAuthClientFeatureSet(selectedClient.client_id, activeSpace.id, featureSetId);
        setGrantedFeatureSetIds(prev => prev.filter(id => id !== featureSetId));
        success('Permission revoked', `"${fsName}" removed from client`);
      } else {
        await grantOAuthClientFeatureSet(selectedClient.client_id, activeSpace.id, featureSetId);
        setGrantedFeatureSetIds(prev => [...prev, featureSetId]);
        success('Permission granted', `"${fsName}" added to client`);
      }
      loadResolvedFeatures(selectedClient.client_id);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to update permission', msg);
    }
  };

  const handleSaveConfig = async () => {
    if (!selectedClient) return;
    
    setIsSaving(true);
    try {
      const settings: UpdateClientRequest = {
        client_alias: editAlias || undefined,
        connection_mode: editMode as 'follow_active' | 'locked' | 'ask_on_change',
        locked_space_id: undefined,
      };
      
      if (editMode === 'locked' && editLockedSpaceId) {
        settings.locked_space_id = editLockedSpaceId;
      }
      
      const updated = await updateOAuthClient(selectedClient.client_id, settings);
      
      setOAuthClients(prev => prev.map(c => 
        c.client_id === updated.client_id ? updated : c
      ));
      
      setSelectedClient(updated);
      success('Client settings saved', `"${updated.client_alias || updated.client_name}" has been updated`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to save settings', msg);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDelete = async (clientId: string) => {
    if (!confirm('Remove this client? All tokens will be revoked.')) return;
    
    const deletedClient = oauthClients.find(c => c.client_id === clientId);
    const clientName = deletedClient?.client_alias || deletedClient?.client_name || 'Client';
    
    try {
      await deleteOAuthClient(clientId);
      setOAuthClients(prev => prev.filter(c => c.client_id !== clientId));
      setSelectedClient(null);
      success('Client removed', `"${clientName}" and its tokens have been revoked`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to remove client', msg);
    }
  };

  const getSpaceName = (spaceId: string | null) => {
    if (!spaceId) return null;
    const space = spaces.find(s => s.id === spaceId);
    return space ? `${space.icon || '📁'} ${space.name}` : null;
  };

  const getModeInfo = (mode: string) => {
    return CONNECTION_MODES.find(m => m.value === mode) || CONNECTION_MODES[0];
  };

  const loadAvailableFeatures = async () => {
    if (!activeSpace) return;
    
    setIsLoadingFeatures(true);
    try {
      const features = await listServerFeatures(activeSpace.id);
      setAvailableFeatures(features.map(f => ({
        id: f.id,
        feature_name: f.feature_name,
        feature_type: f.feature_type,
        description: f.description ?? undefined,
        server_id: f.server_id,
      })));
    } catch (e) {
      console.error('Failed to load available features:', e);
      setAvailableFeatures([]);
    } finally {
      setIsLoadingFeatures(false);
    }
  };

  const loadClientCustomFeatureSet = async (client: OAuthClient) => {
    if (!activeSpace) {
      console.log('Cannot load custom feature set: missing space');
      return;
    }
    
    const clientName = client.client_alias || client.client_name;
    console.log('Finding or creating custom feature set for:', clientName);
    
    try {
      const featureSet = await invoke<FeatureSet>('find_or_create_client_custom_feature_set', {
        clientName,
        spaceId: activeSpace.id,
      });
      
      console.log('Got custom feature set:', featureSet.id);
      setClientCustomFeatureSet(featureSet);
      
      const members = await getFeatureSetMembers(featureSet.id);
      console.log('Loaded feature members:', members.length);
      setIndividualFeatureMembers(members);
      
      if (!grantedFeatureSetIds.includes(featureSet.id)) {
        console.log('Granting custom feature set to client');
        await grantOAuthClientFeatureSet(client.client_id, activeSpace.id, featureSet.id);
        setGrantedFeatureSetIds(prev => [...prev, featureSet.id]);
      }
    } catch (e) {
      console.error('Failed to load/create custom feature set:', e);
      setClientCustomFeatureSet(null);
      setIndividualFeatureMembers([]);
    }
  };

  const toggleIndividualFeature = async (featureId: string) => {
    if (!selectedClient || !activeSpace || !clientCustomFeatureSet) {
      console.error('Missing client, space, or custom feature set');
      return;
    }
    
    console.log('Toggling feature:', featureId);
    
    const isAdded = individualFeatureMembers.some(m => m.member_id === featureId);
    console.log('Feature is currently added:', isAdded);
    
    const feature = availableFeatures.find(f => f.id === featureId);
    const featureName = feature?.feature_name || 'Feature';
    
    try {
      if (isAdded) {
        await removeFeatureFromSet(clientCustomFeatureSet.id, featureId);
        setIndividualFeatureMembers(prev => prev.filter(m => m.member_id !== featureId));
        success('Feature removed', `"${featureName}" removed from client`);
      } else {
        await addFeatureToSet(clientCustomFeatureSet.id, featureId, 'include');
        setIndividualFeatureMembers(prev => [...prev, {
          id: '',
          feature_set_id: clientCustomFeatureSet.id,
          member_type: 'feature',
          member_id: featureId,
          mode: 'include',
        }]);
        success('Feature added', `"${featureName}" added to client`);
      }
      
      await loadResolvedFeatures(selectedClient.client_id);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to toggle feature', msg);
    }
  };

  const getFeatureIcon = (type: string) => {
    switch (type) {
      case 'tool': return '🔧';
      case 'prompt': return '💬';
      case 'resource': return '📄';
      default: return '⚙️';
    }
  };

  const filteredClients = oauthClients.filter(client => {
    if (!searchQuery) return true;
    const query = searchQuery.toLowerCase();
    return (
      client.client_name.toLowerCase().includes(query) ||
      client.client_alias?.toLowerCase().includes(query) ||
      client.client_id.toLowerCase().includes(query)
    );
  });

  const totalFeatures = resolvedFeatures 
    ? resolvedFeatures.tools.length + resolvedFeatures.prompts.length + resolvedFeatures.resources.length 
    : 0;

  return (
    <div className="h-full flex flex-col relative" data-testid="clients-page">
      {/* Header */}
      <div className="flex-shrink-0 p-8 border-b border-[rgb(var(--border-subtle))]">
        <div className="max-w-[2000px] mx-auto">
          <div className="flex items-center justify-between mb-6">
            <div>
              <h1 className="text-3xl font-bold" data-testid="clients-title">Connected Clients</h1>
              <p className="text-base text-[rgb(var(--muted))] mt-2">
                Manage OAuth clients and their permissions
              </p>
            </div>
            <Button 
              variant="ghost" 
              size="md" 
              onClick={refreshOAuthClients}
              disabled={isRefreshingOAuth}
            >
              <RefreshCw className={`h-4 w-4 mr-2 ${isRefreshingOAuth ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </div>

          {/* Search Bar */}
          <div className="relative max-w-3xl">
            <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
            <input
              type="text"
              placeholder="Search clients by name or ID..."
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

      {/* Clients Grid */}
      <div className="flex-1 overflow-auto px-8 py-8">
        <div className="max-w-[2000px] mx-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-64">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : filteredClients.length === 0 ? (
            <Card className="max-w-2xl mx-auto">
              <CardContent className="flex flex-col items-center justify-center py-16">
                <Laptop className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
                <h3 className="text-lg font-medium mb-2">
                  {searchQuery ? 'No clients match your search' : 'No clients connected'}
                </h3>
                <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md">
                  {searchQuery 
                    ? 'Try adjusting your search terms' 
                    : 'Clients like Cursor or VS Code will appear here after connecting via OAuth'
                  }
                </p>
              </CardContent>
            </Card>
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filteredClients.map((client) => {
                const modeInfo = getModeInfo(client.connection_mode);
                const ModeIcon = modeInfo.icon;
                const isSelected = selectedClient?.client_id === client.client_id;
                
                return (
                  <Card 
                    key={client.client_id}
                    className={`cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] ${
                      isSelected ? 'ring-2 ring-primary-500 shadow-lg' : ''
                    }`}
                    onClick={() => openPanel(client)}
                    data-testid={`client-card-${client.client_id.replace(/[^a-zA-Z0-9-_]/g, '_')}`}
                  >
                    <CardContent className="p-6">
                      {/* Client Header */}
                      <div className="flex items-start gap-4 mb-5">
                        <div className="w-16 h-16 flex items-center justify-center text-4xl bg-[rgb(var(--surface))] rounded-xl flex-shrink-0 border border-[rgb(var(--border-subtle))]">
                          <ClientIcon logo_uri={client.logo_uri} client_name={client.client_name} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <h3 className="font-semibold text-lg truncate mb-1.5">
                            {client.client_alias || client.client_name}
                          </h3>
                          {client.client_alias && (
                            <p className="text-sm text-[rgb(var(--muted))] truncate">
                              {client.client_name}
                            </p>
                          )}
                        </div>
                      </div>

                      {/* Status Badge */}
                      <div className="mb-4">
                        <span className={`inline-flex items-center gap-2 text-sm px-3 py-1.5 rounded-full font-medium ${
                          client.has_active_tokens 
                            ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400' 
                            : 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400'
                        }`}>
                          {client.has_active_tokens ? (
                            <><Wifi className="h-4 w-4" /> Active</>
                          ) : (
                            <><WifiOff className="h-4 w-4" /> Inactive</>
                          )}
                        </span>
                      </div>

                      {/* Connection Mode */}
                      <div className="flex items-center gap-2.5 text-sm text-[rgb(var(--foreground))] mb-2">
                        <ModeIcon className={`h-4 w-4 ${modeInfo.color}`} />
                        <span className="font-medium">{modeInfo.label}</span>
                      </div>

                      {/* Locked Space Info */}
                      {client.connection_mode === 'locked' && client.locked_space_id && (
                        <div className="text-sm text-[rgb(var(--muted))] truncate mt-1 pl-6">
                          {getSpaceName(client.locked_space_id)}
                        </div>
                      )}
                    </CardContent>
                  </Card>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Overlay backdrop when panel is open */}
      {selectedClient && (
        <div 
          className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
          onClick={() => setSelectedClient(null)}
        />
      )}

      {/* Slide-out Panel */}
      {selectedClient && (
        <div className="fixed right-0 top-0 bottom-0 w-full max-w-[45%] min-w-[600px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50">
          {/* Panel Header - Compact */}
          <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
            <div className="flex items-start justify-between mb-3">
              <div className="flex items-center gap-3 flex-1 min-w-0">
                <div className="w-10 h-10 flex items-center justify-center text-2xl bg-[rgb(var(--background))] rounded-lg flex-shrink-0">
                  <ClientIcon logo_uri={selectedClient.logo_uri} client_name={selectedClient.client_name} />
                </div>
                <div className="flex-1 min-w-0">
                  <h2 className="text-lg font-bold truncate">
                    {selectedClient.client_alias || selectedClient.client_name}
                  </h2>
                  {selectedClient.client_alias && (
                    <p className="text-xs text-[rgb(var(--muted))] truncate">
                      {selectedClient.client_name}
                    </p>
                  )}
                </div>
              </div>
              <button
                onClick={() => setSelectedClient(null)}
                className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            {/* Quick Status */}
            <div className="flex items-center gap-2 flex-wrap">
              <span className={`inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full font-medium ${
                selectedClient.has_active_tokens 
                  ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400' 
                  : 'bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400'
              }`}>
                {selectedClient.has_active_tokens ? (
                  <><Wifi className="h-3 w-3" /> Connected</>
                ) : (
                  <><WifiOff className="h-3 w-3" /> Inactive</>
                )}
              </span>
              {selectedClient.software_version && (
                <span className="text-xs text-[rgb(var(--muted))] px-2.5 py-1 bg-[rgb(var(--background))] rounded-full">
                  v{selectedClient.software_version}
                </span>
              )}
            </div>
          </div>

          {/* Scrollable Content */}
          <div className="flex-1 overflow-y-auto">
            <div className="p-6 space-y-5">
              {/* Quick Settings Section */}
              <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden transition-all hover:border-primary-200 dark:hover:border-primary-800">
                <button
                  onClick={() => toggleSection('quickSettings')}
                  className={`w-full flex items-center justify-between p-4 transition-all ${
                    expandedSections.quickSettings 
                      ? 'bg-gradient-to-r from-primary-50 to-primary-100 dark:from-primary-900/20 dark:to-primary-800/20'
                      : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div className={`p-2 rounded-lg ${
                      expandedSections.quickSettings
                        ? 'bg-primary-500 text-white'
                        : 'bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400'
                    }`}>
                      <Zap className="h-5 w-5" />
                    </div>
                    <span className="font-semibold text-base">Quick Settings</span>
                  </div>
                  {expandedSections.quickSettings ? (
                    <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                  ) : (
                    <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                  )}
                </button>

                {expandedSections.quickSettings && (
                  <div className="p-4 space-y-4 border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                    {/* Display Name */}
                    <div>
                      <label className="block text-xs font-medium mb-1.5 text-[rgb(var(--muted))]">
                        Display Name
                      </label>
                      <input
                        type="text"
                        value={editAlias}
                        onChange={(e) => setEditAlias(e.target.value)}
                        placeholder={selectedClient.client_name}
                        className="w-full px-3 py-2 text-sm bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
                      />
                    </div>

                    {/* Connection Mode */}
                    <div>
                      <label className="block text-xs font-medium mb-1.5 text-[rgb(var(--muted))]">
                        Connection Mode
                      </label>
                      <select
                        value={editMode}
                        onChange={(e) => setEditMode(e.target.value)}
                        className="w-full px-3 py-2 text-sm bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
                      >
                        {CONNECTION_MODES.map((mode) => (
                          <option key={mode.value} value={mode.value}>
                            {mode.label}
                          </option>
                        ))}
                      </select>
                    </div>

                    {/* Locked Space Selection */}
                    {editMode === 'locked' && (
                      <div className="animate-in slide-in-from-top duration-200">
                        <label className="block text-xs font-medium mb-1.5 text-[rgb(var(--muted))]">
                          Locked Workspace
                        </label>
                        <select
                          value={editLockedSpaceId}
                          onChange={(e) => setEditLockedSpaceId(e.target.value)}
                          className="w-full px-3 py-2 text-sm bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
                        >
                          <option value="">Select a workspace...</option>
                          {spaces.map((space) => (
                            <option key={space.id} value={space.id}>
                              {space.icon || '📁'} {space.name}
                            </option>
                          ))}
                        </select>
                      </div>
                    )}

                    {/* Save Button */}
                    <Button
                      onClick={handleSaveConfig}
                      disabled={isSaving}
                      size="sm"
                      className="w-full"
                    >
                      {isSaving ? (
                        <><Loader2 className="h-4 w-4 mr-2 animate-spin" /> Saving...</>
                      ) : (
                        <>
                          <Check className="h-4 w-4 mr-2" />
                          Save Changes
                        </>
                      )}
                    </Button>
                  </div>
                )}
              </div>

              {/* Permissions Section */}
              <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden transition-all hover:border-primary-200 dark:hover:border-primary-800">
                <button
                  onClick={() => toggleSection('permissions')}
                  className={`w-full flex items-center justify-between p-4 transition-all ${
                    expandedSections.permissions 
                      ? 'bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20' 
                      : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div className={`p-2 rounded-lg ${
                      expandedSections.permissions
                        ? 'bg-blue-500 text-white'
                        : 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                    }`}>
                      <Shield className="h-5 w-5" />
                    </div>
                    <span className="font-semibold text-base">Permissions</span>
                    {selectedClient.connection_mode === 'locked' && selectedClient.locked_space_id !== activeSpace?.id && (
                      <Lock className="h-4 w-4 text-orange-500" />
                    )}
                  </div>
                  {expandedSections.permissions ? (
                    <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                  ) : (
                    <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                  )}
                </button>

                {expandedSections.permissions && (
                  <div className="p-4 space-y-4 border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                    {/* Context Warning */}
                    {selectedClient.connection_mode === 'locked' && selectedClient.locked_space_id !== activeSpace?.id ? (
                      <div className="p-3 bg-orange-50 dark:bg-orange-900/20 border border-orange-200 dark:border-orange-800 rounded-lg">
                        <div className="flex items-start gap-2">
                          <Lock className="h-4 w-4 text-orange-500 flex-shrink-0 mt-0.5" />
                          <div>
                            <p className="text-xs font-medium text-orange-900 dark:text-orange-100">
                              Locked to {getSpaceName(selectedClient.locked_space_id)}
                            </p>
                            <p className="text-xs text-orange-700 dark:text-orange-300 mt-1">
                              Switch spaces or change connection mode to manage permissions
                            </p>
                          </div>
                        </div>
                      </div>
                    ) : (
                      <>
                        {/* Space Context */}
                        {activeSpace && (
                          <div className="p-2.5 bg-primary-50 dark:bg-primary-900/20 border border-primary-200 dark:border-primary-800 rounded-lg">
                            <div className="flex items-center gap-2 text-xs">
                              <span className="text-[rgb(var(--muted))]">Managing:</span>
                              <span className="font-medium text-primary-700 dark:text-primary-300">
                                {activeSpace.icon || '📁'} {activeSpace.name}
                              </span>
                            </div>
                          </div>
                        )}

                        {/* Feature Sets */}
                        {isLoadingGrants ? (
                          <div className="flex items-center justify-center py-6">
                            <Loader2 className="h-5 w-5 animate-spin text-primary-500" />
                          </div>
                        ) : (
                          <div className="space-y-2">
                            <div className="text-xs font-medium text-[rgb(var(--muted))] mb-2">
                              Feature Sets
                            </div>
                            {availableFeatureSets
                              .filter(fs => !fs.name.endsWith(' - Custom'))
                              .slice(0, 5)
                              .map((fs) => {
                                const isGranted = grantedFeatureSetIds.includes(fs.id);
                                const isDefault = fs.feature_set_type === 'default';
                                const isDisabled = isDefault;
                                
                                return (
                                  <button
                                    key={fs.id}
                                    onClick={() => !isDisabled && toggleFeatureSetGrant(fs.id)}
                                    disabled={isDisabled}
                                    className={`w-full flex items-center gap-2.5 p-2.5 rounded-lg border transition-all text-left ${
                                      isDisabled
                                        ? 'opacity-50 cursor-not-allowed'
                                        : 'hover:border-[rgb(var(--border-hover))] cursor-pointer'
                                    } ${
                                      isGranted
                                        ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20'
                                        : 'border-[rgb(var(--border))]'
                                    }`}
                                  >
                                    <div className={`flex-shrink-0 w-4 h-4 rounded border-2 flex items-center justify-center ${
                                      isGranted 
                                        ? 'bg-primary-500 border-primary-500' 
                                        : 'border-[rgb(var(--border))]'
                                    }`}>
                                      {isGranted && <Check className="h-3 w-3 text-white" />}
                                    </div>
                                    
                                    <div className="flex-1 min-w-0">
                                      <div className="flex items-center gap-1.5">
                                        <span className="text-xs">{fs.icon || '📦'}</span>
                                        <span className="font-medium text-xs truncate">
                                          {fs.name}
                                        </span>
                                        {isDefault && (
                                          <span className="text-[10px] text-[rgb(var(--muted))]">
                                            (auto)
                                          </span>
                                        )}
                                      </div>
                                      {fs.description && (
                                        <div className="text-[10px] text-[rgb(var(--muted))] truncate mt-0.5">
                                          {fs.description}
                                        </div>
                                      )}
                                    </div>
                                  </button>
                                );
                              })}
                          </div>
                        )}

                        {/* Advanced Permissions Toggle */}
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setExpandedSections(prev => ({ 
                              ...prev, 
                              advancedPermissions: !prev.advancedPermissions 
                            }));
                          }}
                          className="w-full flex items-center justify-between p-2 hover:bg-[rgb(var(--surface-hover))] rounded-lg transition-colors text-xs font-medium text-[rgb(var(--muted))]"
                        >
                          <span>Advanced: Individual Features</span>
                          {expandedSections.advancedPermissions ? (
                            <ChevronDown className="h-3.5 w-3.5" />
                          ) : (
                            <ChevronRight className="h-3.5 w-3.5" />
                          )}
                        </button>

                        {/* Advanced Permissions Content */}
                        {expandedSections.advancedPermissions && (
                          <div className="space-y-2 pl-2 animate-in slide-in-from-top duration-200">
                            {isLoadingFeatures ? (
                              <div className="flex items-center justify-center py-4">
                                <Loader2 className="h-4 w-4 animate-spin text-primary-500" />
                              </div>
                            ) : (() => {
                              const serverGroups = availableFeatures.reduce((acc, feature) => {
                                if (!acc[feature.server_id]) {
                                  acc[feature.server_id] = [];
                                }
                                acc[feature.server_id].push(feature);
                                return acc;
                              }, {} as Record<string, typeof availableFeatures>);

                              return (
                                <div className="space-y-1.5">
                                  {Object.entries(serverGroups).map(([serverId, features]) => {
                                    const isExpanded = expandedServers.has(serverId);
                                    const selectedCount = features.filter(f => 
                                      individualFeatureMembers.some(m => m.member_id === f.id)
                                    ).length;
                                    
                                    return (
                                      <div key={serverId} className="border border-[rgb(var(--border))] rounded-lg overflow-hidden">
                                        <button
                                          onClick={() => {
                                            const newExpanded = new Set(expandedServers);
                                            if (isExpanded) {
                                              newExpanded.delete(serverId);
                                            } else {
                                              newExpanded.add(serverId);
                                            }
                                            setExpandedServers(newExpanded);
                                          }}
                                          className="w-full flex items-center justify-between p-2 bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                                        >
                                          <div className="flex items-center gap-2">
                                            <span className="text-xs font-medium truncate">{serverId}</span>
                                            {selectedCount > 0 && (
                                              <span className="text-[10px] bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 px-1.5 py-0.5 rounded-full font-medium">
                                                {selectedCount}
                                              </span>
                                            )}
                                          </div>
                                          {isExpanded ? (
                                            <ChevronDown className="h-3.5 w-3.5 text-[rgb(var(--muted))]" />
                                          ) : (
                                            <ChevronRight className="h-3.5 w-3.5 text-[rgb(var(--muted))]" />
                                          )}
                                        </button>
                                        
                                        {isExpanded && (
                                          <div className="p-1.5 space-y-1 max-h-48 overflow-y-auto">
                                            {features.map((feature) => {
                                              const isAdded = individualFeatureMembers.some(m => m.member_id === feature.id);
                                              
                                              return (
                                                <button
                                                  key={feature.id}
                                                  onClick={() => toggleIndividualFeature(feature.id)}
                                                  className={`w-full flex items-center gap-2 p-1.5 rounded border transition-colors ${
                                                    isAdded
                                                      ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20'
                                                      : 'border-transparent hover:border-[rgb(var(--border))]'
                                                  }`}
                                                >
                                                  <div className={`flex-shrink-0 w-3 h-3 rounded border flex items-center justify-center ${
                                                    isAdded
                                                      ? 'bg-primary-500 border-primary-500'
                                                      : 'border-[rgb(var(--border))]'
                                                  }`}>
                                                    {isAdded && <Check className="h-2 w-2 text-white" />}
                                                  </div>
                                                  
                                                  <span className="text-[10px]">
                                                    {getFeatureIcon(feature.feature_type)}
                                                  </span>
                                                  
                                                  <span className="text-xs flex-1 text-left truncate">
                                                    {feature.feature_name}
                                                  </span>
                                                </button>
                                              );
                                            })}
                                          </div>
                                        )}
                                      </div>
                                    );
                                  })}
                                </div>
                              );
                            })()}
                          </div>
                        )}
                      </>
                    )}
                  </div>
                )}
              </div>

              {/* Effective Features Section */}
              <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden transition-all hover:border-primary-200 dark:hover:border-primary-800">
                <button
                  onClick={() => toggleSection('effectiveFeatures')}
                  className={`w-full flex items-center justify-between p-4 transition-all ${
                    expandedSections.effectiveFeatures 
                      ? 'bg-gradient-to-r from-purple-50 to-pink-50 dark:from-purple-900/20 dark:to-pink-900/20' 
                      : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div className={`p-2 rounded-lg ${
                      expandedSections.effectiveFeatures
                        ? 'bg-purple-500 text-white'
                        : 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400'
                    }`}>
                      <Layers className="h-5 w-5" />
                    </div>
                    <span className="font-semibold text-base">Effective Features</span>
                    {totalFeatures > 0 && (
                      <span className="text-xs bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 px-2 py-0.5 rounded-full font-medium">
                        {totalFeatures}
                      </span>
                    )}
                  </div>
                  {expandedSections.effectiveFeatures ? (
                    <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                  ) : (
                    <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                  )}
                </button>

                {expandedSections.effectiveFeatures && (
                  <div className="p-4 space-y-3 border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                    {isLoadingResolvedFeatures ? (
                      <div className="flex items-center justify-center py-6">
                        <Loader2 className="h-5 w-5 animate-spin text-primary-500" />
                      </div>
                    ) : !resolvedFeatures || totalFeatures === 0 ? (
                      <div className="text-center py-6">
                        <Layers className="h-8 w-8 mx-auto mb-2 text-[rgb(var(--muted))]" />
                        <p className="text-xs text-[rgb(var(--muted))]">
                          No features granted yet
                        </p>
                      </div>
                    ) : (
                      <div className="space-y-2">
                        {/* Tools */}
                        {resolvedFeatures.tools.length > 0 && (
                          <div className="border border-blue-200 dark:border-blue-800 rounded-lg overflow-hidden">
                            <button
                              onClick={() => setExpandedFeatureTypes(prev => ({ ...prev, tools: !prev.tools }))}
                              className="w-full flex items-center justify-between p-2 bg-blue-50 dark:bg-blue-900/20 hover:bg-blue-100 dark:hover:bg-blue-900/30 transition-colors"
                            >
                              <div className="flex items-center gap-2">
                                <span className="text-sm">{getFeatureIcon('tool')}</span>
                                <span className="text-xs font-medium text-blue-700 dark:text-blue-300">
                                  Tools
                                </span>
                                <span className="text-[10px] bg-blue-200 dark:bg-blue-800 text-blue-700 dark:text-blue-300 px-1.5 py-0.5 rounded-full font-medium">
                                  {resolvedFeatures.tools.length}
                                </span>
                              </div>
                              {expandedFeatureTypes.tools ? (
                                <ChevronDown className="h-3.5 w-3.5 text-blue-600 dark:text-blue-400" />
                              ) : (
                                <ChevronRight className="h-3.5 w-3.5 text-blue-600 dark:text-blue-400" />
                              )}
                            </button>
                            {expandedFeatureTypes.tools && (
                              <div className="p-2 space-y-1 max-h-48 overflow-y-auto">
                                {resolvedFeatures.tools.map((tool) => (
                                  <div
                                    key={tool.name}
                                    className="p-2 rounded bg-blue-50 dark:bg-blue-900/10 border border-blue-100 dark:border-blue-900/30"
                                  >
                                    <div className="font-medium text-xs text-blue-900 dark:text-blue-100 truncate">
                                      {tool.name}
                                    </div>
                                    {tool.description && (
                                      <div className="text-[10px] text-blue-700 dark:text-blue-300 mt-0.5 line-clamp-2">
                                        {tool.description}
                                      </div>
                                    )}
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        )}

                        {/* Prompts */}
                        {resolvedFeatures.prompts.length > 0 && (
                          <div className="border border-purple-200 dark:border-purple-800 rounded-lg overflow-hidden">
                            <button
                              onClick={() => setExpandedFeatureTypes(prev => ({ ...prev, prompts: !prev.prompts }))}
                              className="w-full flex items-center justify-between p-2 bg-purple-50 dark:bg-purple-900/20 hover:bg-purple-100 dark:hover:bg-purple-900/30 transition-colors"
                            >
                              <div className="flex items-center gap-2">
                                <span className="text-sm">{getFeatureIcon('prompt')}</span>
                                <span className="text-xs font-medium text-purple-700 dark:text-purple-300">
                                  Prompts
                                </span>
                                <span className="text-[10px] bg-purple-200 dark:bg-purple-800 text-purple-700 dark:text-purple-300 px-1.5 py-0.5 rounded-full font-medium">
                                  {resolvedFeatures.prompts.length}
                                </span>
                              </div>
                              {expandedFeatureTypes.prompts ? (
                                <ChevronDown className="h-3.5 w-3.5 text-purple-600 dark:text-purple-400" />
                              ) : (
                                <ChevronRight className="h-3.5 w-3.5 text-purple-600 dark:text-purple-400" />
                              )}
                            </button>
                            {expandedFeatureTypes.prompts && (
                              <div className="p-2 space-y-1 max-h-48 overflow-y-auto">
                                {resolvedFeatures.prompts.map((prompt) => (
                                  <div
                                    key={prompt.name}
                                    className="p-2 rounded bg-purple-50 dark:bg-purple-900/10 border border-purple-100 dark:border-purple-900/30"
                                  >
                                    <div className="font-medium text-xs text-purple-900 dark:text-purple-100 truncate">
                                      {prompt.name}
                                    </div>
                                    {prompt.description && (
                                      <div className="text-[10px] text-purple-700 dark:text-purple-300 mt-0.5 line-clamp-2">
                                        {prompt.description}
                                      </div>
                                    )}
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        )}

                        {/* Resources */}
                        {resolvedFeatures.resources.length > 0 && (
                          <div className="border border-green-200 dark:border-green-800 rounded-lg overflow-hidden">
                            <button
                              onClick={() => setExpandedFeatureTypes(prev => ({ ...prev, resources: !prev.resources }))}
                              className="w-full flex items-center justify-between p-2 bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors"
                            >
                              <div className="flex items-center gap-2">
                                <span className="text-sm">{getFeatureIcon('resource')}</span>
                                <span className="text-xs font-medium text-green-700 dark:text-green-300">
                                  Resources
                                </span>
                                <span className="text-[10px] bg-green-200 dark:bg-green-800 text-green-700 dark:text-green-300 px-1.5 py-0.5 rounded-full font-medium">
                                  {resolvedFeatures.resources.length}
                                </span>
                              </div>
                              {expandedFeatureTypes.resources ? (
                                <ChevronDown className="h-3.5 w-3.5 text-green-600 dark:text-green-400" />
                              ) : (
                                <ChevronRight className="h-3.5 w-3.5 text-green-600 dark:text-green-400" />
                              )}
                            </button>
                            {expandedFeatureTypes.resources && (
                              <div className="p-2 space-y-1 max-h-48 overflow-y-auto">
                                {resolvedFeatures.resources.map((resource) => (
                                  <div
                                    key={resource.name}
                                    className="p-2 rounded bg-green-50 dark:bg-green-900/10 border border-green-100 dark:border-green-900/30"
                                  >
                                    <div className="font-medium text-xs text-green-900 dark:text-green-100 truncate">
                                      {resource.name}
                                    </div>
                                    {resource.description && (
                                      <div className="text-[10px] text-green-700 dark:text-green-300 mt-0.5 line-clamp-2">
                                        {resource.description}
                                      </div>
                                    )}
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {/* Client Info Section */}
              <div className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden transition-all hover:border-primary-200 dark:hover:border-primary-800">
                <button
                  onClick={() => toggleSection('clientInfo')}
                  className={`w-full flex items-center justify-between p-4 transition-all ${
                    expandedSections.clientInfo 
                      ? 'bg-gradient-to-r from-primary-50 to-primary-100/50 dark:from-primary-900/10 dark:to-primary-800/10'
                      : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div className={`p-2 rounded-lg ${
                      expandedSections.clientInfo
                        ? 'bg-gray-500 text-white'
                        : 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400'
                    }`}>
                      <Settings className="h-5 w-5" />
                    </div>
                    <span className="font-semibold text-base">Client Information</span>
                  </div>
                  {expandedSections.clientInfo ? (
                    <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                  ) : (
                    <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                  )}
                </button>

                {expandedSections.clientInfo && (
                  <div className="p-4 space-y-3 border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))]">
                    <div className="grid grid-cols-2 gap-2">
                      <div className="p-2 bg-[rgb(var(--surface))] rounded-lg">
                        <div className="text-[10px] text-[rgb(var(--muted))] mb-1">Client ID</div>
                        <div className="font-mono text-[10px] break-all">{selectedClient.client_id}</div>
                      </div>
                      <div className="p-2 bg-[rgb(var(--surface))] rounded-lg">
                        <div className="text-[10px] text-[rgb(var(--muted))] mb-1">Type</div>
                        <div className="text-[10px]">{selectedClient.registration_type || 'dynamic'}</div>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Panel Footer - Sticky */}
          <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => handleDelete(selectedClient.client_id)}
              className="w-full text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20"
            >
              <Trash2 className="h-4 w-4 mr-2" />
              Remove Client
            </Button>
          </div>
        </div>
      )}

      <ToastContainer toasts={toasts} onClose={dismiss} />
    </div>
  );
}
