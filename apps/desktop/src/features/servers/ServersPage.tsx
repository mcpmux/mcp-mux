/**
 * Servers page for managing installed MCP servers and their connections.
 * 
 * Uses event-driven ServerManager for:
 * - Real-time status updates via Tauri events
 * - Connect/Reconnect/Cancel button logic
 * - Auth progress display during OAuth
 */

import { useEffect, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import i18n from '@/i18n';
import { pickPath } from '@/lib/backend/shell';
import { isTauri } from '@/lib/backend/data/transport';
import {
  ChevronDown,
  ChevronRight,
  Wrench,
  MessageSquare,
  FileText,
  Loader2,
  Clock,
  FolderOpen,
  UnfoldVertical,
  FoldVertical,
  Search,
} from 'lucide-react';
import { Button, SearchField } from '@mcpmux/ui';
import { ServerActionMenu } from './ServerActionMenu';
import {
  isPackageManagedTransport,
  isValidSemver,
  resolveCurrentPackageVersion,
  shouldShowPackageUpdate,
  getUpdatePolicyOptions,
} from './server-update-policy.helpers';
import { ServerEnabledToggle } from './ServerEnabledToggle';
import { CloneAccountModal } from './CloneAccountModal';
import { AddServerMenu } from './AddServerMenu';
import { ServersFiltersPopover } from './ServersFiltersPopover';
import { ServersCountSummary } from './ServersCountSummary';
import { UninstallSourceWithClonesDialog } from './UninstallSourceWithClonesDialog';
import type { ServerViewModel, ServerDefinition, InstalledServerState, InputDefinition } from '../../types/registry';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { listServerFeatures, listServerFeaturesByServer } from '@/lib/api/serverFeatures';
import {
  computeServerCountSummary,
  groupFeaturesByServerId,
  serverMatchesFilters,
  type ServerActionKey,
  type StatusFilterKey,
  type TransportFilter,
} from './servers-page.helpers';
import { resolveInstalledDisplayName } from './server-display-name.helpers';
import type { ConnectionStatus, ServerStatusResponse } from '@/lib/api/serverManager';
import { getServerStatuses as fetchServerStatuses } from '@/lib/api/serverManager';
import { checkServerVersion } from '@/lib/api/settings';
import type { UpdatePolicy } from '@/lib/api/settings';
import { useViewSpace, useNavigateTo, usePendingServersFilter, useSetPendingServersFilter } from '@/stores';
import { useServerManager } from '@/hooks/useServerManager';
import { useGatewayControl } from '@/features/gateway/useGatewayControl';
import { useGatewayEvents, useDomainEvents } from '@/hooks/useDomainEvents';
import type {
  GatewayChangedPayload,
  ServerChangedPayload,
  ServerUpdateAvailablePayload,
} from '@/hooks/useDomainEvents';
import type { FeaturesUpdatedEvent } from '@/lib/api/serverManager';
import { ServerLogViewer } from '@/components/ServerLogViewer';
import { ConfigEditorModal } from '@/components/ConfigEditorModal';
import { ServerDefinitionModal } from '@/components/ServerDefinitionModal';
import { SourceBadge } from '@/components/SourceBadge';
import type { ClonedInstalledServer } from '@/lib/api/serverClone';
import { listCloneDependents } from '@/lib/api/serverClone';

/** Server view model extended with optional clone lineage from the backend. */
type ServerViewModelWithClone = ServerViewModel;

/**
 * Read clone lineage from an installed-server row when the TS type has not caught up yet.
 */
function getInstalledCloneLineage(state: InstalledServerState): string | undefined {
  const clonedFrom = (state as InstalledServerState & { cloned_from?: string | null }).cloned_from;
  return clonedFrom ?? undefined;
}

/**
 * Whether the overflow menu should offer "Add another account…".
 */
function canCloneServer(server: ServerViewModelWithClone): boolean {
  if (server.cloned_from) {
    return false;
  }

  const sourceType = server.installation_source?.type;
  return sourceType === 'registry' || sourceType === 'manual_entry';
}

// Helper to merge definitions with states (same as registryStore)
function mergeDefinitionsWithStates(
  definitions: ServerDefinition[],
  states: InstalledServerState[]
): ServerViewModelWithClone[] {
  const stateMap = new Map(states.map(s => [s.server_id, s]));
  
  return definitions.map(def => {
    const state = stateMap.get(def.id);

    const inputs = def.transport.metadata?.inputs ?? [];
    const inputValues = state?.input_values ?? {};
    const missing_required_inputs = inputs.some((input: InputDefinition) =>
      input.required && !inputValues[input.id]
    );

    // Runtime status is in-memory only and comes from ServerManager.
    // Do not infer `connecting` from persisted `enabled`; custom/offline servers can
    // otherwise stay stuck in a synthetic Connecting state forever.
    const connection_status = 'disconnected';
    const displayName = state ? resolveInstalledDisplayName(state, def) : def.name;

    return {
      ...def,
      name: displayName,
      is_installed: !!state,
      enabled: state?.enabled ?? false,
      oauth_connected: state?.oauth_connected ?? false,
      input_values: inputValues,
      connection_status,
      missing_required_inputs,
      last_error: null,
      created_at: state?.created_at,
      installation_source: state?.source,
      cloned_from: state ? getInstalledCloneLineage(state) : undefined,
      env_overrides: state?.env_overrides ?? {},
      args_append: state?.args_append ?? [],
      extra_headers: state?.extra_headers ?? {},
      default_params: state?.default_params ?? {},
      update_policy: state?.update_policy ?? 'notify',
      pinned_version: state?.pinned_version ?? null,
      latest_available_version: state?.latest_available_version ?? null,
      current_version: state?.current_version ?? null,
      version_checked_at: state?.version_checked_at ?? null,
    } as ServerViewModel;
  });
}

// Helper to create ServerViewModel from installed state when registry is unavailable
// Uses cached_definition if available (proper offline support), otherwise falls back to minimal data
function createOfflineServerViewModel(state: InstalledServerState): ServerViewModelWithClone {
  if (state.cached_definition) {
    try {
      const definition: ServerDefinition = JSON.parse(state.cached_definition);
      const inputValues = state.input_values;
      const requiredInputs = definition.transport.metadata?.inputs?.filter((i) => i.required) || [];
      const missing_required_inputs = requiredInputs.some((input) => !inputValues[input.id]);
      const connection_status = 'disconnected';

      return {
        ...definition,
        name: resolveInstalledDisplayName(state, definition),
        is_installed: true,
        enabled: state.enabled,
        oauth_connected: state.oauth_connected,
        input_values: inputValues,
        connection_status,
        missing_required_inputs,
        last_error: null,
        created_at: state.created_at,
        installation_source: state.source,
        cloned_from: getInstalledCloneLineage(state),
        env_overrides: state.env_overrides ?? {},
        args_append: state.args_append ?? [],
        extra_headers: state.extra_headers ?? {},
        default_params: state.default_params ?? {},
        update_policy: state.update_policy ?? 'notify',
        pinned_version: state.pinned_version ?? null,
        latest_available_version: state.latest_available_version ?? null,
        current_version: state.current_version ?? null,
        version_checked_at: state.version_checked_at ?? null,
      } as ServerViewModel;
    } catch (e) {
      console.warn('[ServersPage] Failed to parse cached_definition, using minimal fallback:', e);
    }
  }

  return {
    id: state.server_id,
    name: resolveInstalledDisplayName(state),
    description: i18n.t('servers:offline.noCachedDefinition'),
    alias: null,
    icon: null,
    categories: [],
    publisher: null,
    source: { type: 'Bundled' },
    auth: null,
    transport: {
      type: 'stdio',
      command: 'unknown',
      args: [],
      env: {},
      metadata: { inputs: [] },
    },
    is_installed: true,
    enabled: state.enabled,
    oauth_connected: state.oauth_connected,
    input_values: state.input_values,
    connection_status: 'disconnected',
    missing_required_inputs: false,
    last_error: null,
    created_at: state.created_at,
    installation_source: state.source,
    cloned_from: getInstalledCloneLineage(state),
    env_overrides: state.env_overrides ?? {},
    args_append: state.args_append ?? [],
    extra_headers: state.extra_headers ?? {},
    default_params: state.default_params ?? {},
  } as ServerViewModel;
}


interface ConfigModalState {
  open: boolean;
  server: ServerViewModel | null;
  inputValues: Record<string, string>;
  /** If true, saving will also enable the server (from Enable flow) */
  enableOnSave?: boolean;
  /** Additional environment variable overrides */
  envOverrides: Record<string, string>;
  /** Additional arguments to append (stdio only) */
  argsAppend: string[];
  /** Extra HTTP headers (http only) */
  extraHeaders: Record<string, string>;
  /** Default tool-call arguments (JSON textarea). */
  defaultParamsJson: string;
  /** Merge strategy for default_params. */
  defaultParamsStrategy: 'fill' | 'override';
  /** User-supplied display label (empty string = clear override). */
  displayName: string;
  /** Display name when the modal opened — used to detect changes on save. */
  initialDisplayName: string;
  /** Per-server update policy override. */
  updatePolicy: UpdatePolicy;
  initialUpdatePolicy: UpdatePolicy;
  /** Semver pin when policy is `pinned`. */
  pinnedVersion: string;
  initialPinnedVersion: string;
}

export function ServersPage() {
  const { t } = useTranslation(['servers', 'common']);
  const { t: tCommon } = useTranslation('common');
  const updatePolicyOptions = getUpdatePolicyOptions(t);
  const [installedServers, setInstalledServers] = useState<ServerViewModelWithClone[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [transportFilter, setTransportFilter] = useState<TransportFilter>('all');
  const pendingServersFilter = usePendingServersFilter();
  const setPendingServersFilter = useSetPendingServersFilter();
  const [activeStatusFilters, setActiveStatusFilters] = useState<Set<StatusFilterKey>>(
    pendingServersFilter ? new Set([pendingServersFilter as StatusFilterKey]) : new Set()
  );
  const [gatewayRunning, setGatewayRunning] = useState(false);
  const [gatewayUrl, setGatewayUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const gatewayControl = useGatewayControl();
  // Bottom toast notifications
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' | 'info' } | null>(null);
  const [configModal, setConfigModal] = useState<ConfigModalState>({
    open: false,
    server: null,
    inputValues: {},
    envOverrides: {},
    argsAppend: [],
    extraHeaders: {},
    defaultParamsJson: '{}',
    defaultParamsStrategy: 'fill',
    displayName: '',
    initialDisplayName: '',
    updatePolicy: 'notify',
    initialUpdatePolicy: 'notify',
    pinnedVersion: '',
    initialPinnedVersion: '',
  });

  // Features state
  const [serverFeatures, setServerFeatures] = useState<Record<string, ServerFeature[]>>({});
  const [expandedServers, setExpandedServers] = useState<Set<string>>(new Set());
  const [loadingFeatures, setLoadingFeatures] = useState<Set<string>>(new Set());
  
  // Log viewer state
  const [logViewerServer, setLogViewerServer] = useState<{ id: string; name: string } | null>(null);

  // Definition viewer state
  const [definitionServer, setDefinitionServer] = useState<{ id: string; name: string } | null>(null);

  // Clone account wizard state
  const [cloneModalServer, setCloneModalServer] = useState<ServerViewModelWithClone | null>(null);

  // Uninstall source-with-clones confirmation
  const [uninstallClonesDialog, setUninstallClonesDialog] = useState<{
    server: ServerViewModelWithClone;
    dependents: ClonedInstalledServer[];
  } | null>(null);
  
  // Config editor state
  const [editConfigSpace, setEditConfigSpace] = useState<{ id: string; name: string } | null>(null);
  
  const viewSpace = useViewSpace();
  const navigateTo = useNavigateTo();

  // Event-driven server status management
  const {
    statuses: serverStatuses,
    authProgress,
    enable: enableServerV2,
    disable: disableServerV2,
    connect: startAuthV2,
    cancel: cancelAuthV2,
    retry: retryConnectionV2,
  } = useServerManager({
    spaceId: viewSpace?.id || '',
    onFeaturesChange: (event: FeaturesUpdatedEvent) => {
      // Update features when they change
      console.log('[ServersPage] Features updated:', event);
      
      // Flatten features from the event (tools, prompts, resources)
      const allFeatures = [
        ...event.features.tools,
        ...event.features.prompts,
        ...event.features.resources,
      ];
      
      // Update server features state directly from event
      setServerFeatures(prev => ({
        ...prev,
        [event.server_id]: allFeatures,
      }));
      
      // Automatically expand server to show features
      setExpandedServers(prev => new Set(prev).add(event.server_id));
    },
  });
  
  // Helper to get runtime status for a server (from ServerManager events)
  const getRuntimeStatus = useCallback((serverId: string): ConnectionStatus | undefined => {
    return serverStatuses[serverId]?.status;
  }, [serverStatuses]);
  
  // Helper to check if server has connected before
  const hasConnectedBefore = useCallback((serverId: string): boolean => {
    return serverStatuses[serverId]?.has_connected_before ?? false;
  }, [serverStatuses]);
  
  // Helper to get auth progress for a server
  const getAuthRemainingSeconds = useCallback((serverId: string): number | undefined => {
    return authProgress[serverId];
  }, [authProgress]);

  // Show toast notification
  const showToast = useCallback((message: string, type: 'success' | 'error' | 'info' = 'info') => {
    setToast({ message, type });
    setTimeout(() => setToast(null), 5000);
  }, []);

  const loadData = useCallback(async () => {
    try {
      setIsLoading(true);

      // Use allSettled so we can show installed servers even if registry is offline
      const [installedResult, gatewayResult, definitionsResult, statusesResult] =
        await Promise.allSettled([
          import('@/lib/api/registry').then((m) => m.listInstalledServers(viewSpace?.id)),
          import('@/lib/api/gateway').then((m) => m.getGatewayStatus(viewSpace?.id)),
          import('@/lib/api/registry').then((m) => m.discoverServers()),
          viewSpace?.id
            ? fetchServerStatuses(viewSpace.id)
            : Promise.resolve({} as Record<string, ServerStatusResponse>),
        ]);

      // Extract values, using fallbacks for failures
      const installed = installedResult.status === 'fulfilled' ? installedResult.value : [];
      const gateway =
        gatewayResult.status === 'fulfilled'
          ? gatewayResult.value
          : { running: false, url: null };
      const definitions =
        definitionsResult.status === 'fulfilled' ? definitionsResult.value : [];
      const runtimeStatuses: Record<string, ServerStatusResponse> =
        statusesResult.status === 'fulfilled' ? statusesResult.value : {};

      // Log if registry is offline but we have installed servers
      if (definitionsResult.status === 'rejected' && installed.length > 0) {
        console.warn(
          '[ServersPage] Registry offline, showing installed servers with cached/minimal info'
        );
        showToast(t('offline.registryOffline'), 'info');
      }

      // Merge definitions with installed states
      // If definitions are missing, create minimal ServerViewModels from installed states
      let mergedServers: ServerViewModelWithClone[];

      if (definitions.length > 0) {
        // Normal case: merge definitions with states
        const allMerged = mergeDefinitionsWithStates(definitions, installed);
        mergedServers = allMerged.filter((s) => s.is_installed);

        // Handle installed servers not present in registry definitions
        // (e.g., registry changed, using different registry, or servers installed from user config)
        const matchedServerIds = new Set(mergedServers.map((s) => s.id));
        const unmatchedInstalled = installed.filter((s) => !matchedServerIds.has(s.server_id));
        if (unmatchedInstalled.length > 0) {
          const offlineViewModels = unmatchedInstalled.map((state) =>
            createOfflineServerViewModel(state)
          );
          mergedServers = [...mergedServers, ...offlineViewModels];
        }
      } else {
        // Offline case: create minimal view models from installed states only
        mergedServers = installed.map((state) => createOfflineServerViewModel(state));
      }

      // Apply runtime statuses from ServerManager to fix initial connection_status
      // (the view-model builders seed 'disconnected'; ServerManager owns the real
      // runtime status, which arrives via events)
      const mapStatus = (s: ConnectionStatus): ServerViewModel['connection_status'] => {
        if (s === 'refreshing' || s === 'authenticating') return 'connecting';
        return s;
      };
      for (const server of mergedServers) {
        const runtime = runtimeStatuses[server.id];
        if (runtime) {
          server.connection_status = mapStatus(runtime.status);
          server.last_error = runtime.message || null;
        }
      }

      // Sort by installation time (newest first)
      mergedServers.sort((a, b) => {
        const dateA = new Date(a.created_at || 0).getTime();
        const dateB = new Date(b.created_at || 0).getTime();
        return dateB - dateA;
      });

      setInstalledServers(mergedServers);
      setGatewayRunning(gateway.running);
      setGatewayUrl(gateway.url);

      if (viewSpace?.id) {
        try {
          const allFeatures = await listServerFeatures(viewSpace.id);
          setServerFeatures(groupFeaturesByServerId(allFeatures));
        } catch (featureError) {
          console.warn('[ServersPage] Failed to load server features for search:', featureError);
        }
      }
    } catch (e) {
      console.error('Failed to load data:', e);
    } finally {
      setIsLoading(false);
    }
  }, [viewSpace?.id, showToast, t]);

  // Clear any pending filter that was consumed during initialisation
  useEffect(() => {
    if (pendingServersFilter) {
      setPendingServersFilter(null);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    void loadData();
  }, [loadData]);

  useEffect(() => {
    setServerFeatures({});
    setExpandedServers(new Set());
    setLoadingFeatures(new Set());
  }, [viewSpace?.id]);

  // Subscribe to gateway events for reactive updates (no polling!)
  useGatewayEvents((payload: GatewayChangedPayload) => {
    if (payload.action === 'started') {
      setGatewayRunning(true);
      setGatewayUrl(payload.url || null);
      // Status changes are handled via per-space events
    } else if (payload.action === 'stopped') {
      setGatewayRunning(false);
      setGatewayUrl(null);
    }
  });

  // Subscribe to server lifecycle events (install/uninstall)
  const { subscribe } = useDomainEvents();
  useEffect(() => {
    return subscribe('server-changed', (payload: ServerChangedPayload) => {
      if (!viewSpace || payload.space_id !== viewSpace.id) {
        return;
      }
      
      // Reload server list when a server is installed or uninstalled
      if (payload.action === 'installed' || payload.action === 'uninstalled') {
        console.log('[ServersPage] Server lifecycle event:', payload.action, payload.server_id);
        void loadData();
      }
    });
  }, [loadData, subscribe, viewSpace]);

  useEffect(() => {
    return subscribe('server-update-available', (payload: ServerUpdateAvailablePayload) => {
      if (!viewSpace || payload.space_id !== viewSpace.id) {
        return;
      }

      setInstalledServers((current) =>
        current.map((server) => {
          if (server.id !== payload.server_id) {
            return server;
          }
          return {
            ...server,
            latest_available_version: payload.latest_version ?? server.latest_available_version,
            current_version: payload.current_version ?? server.current_version,
            version_checked_at: new Date().toISOString(),
          };
        })
      );
    });
  }, [subscribe, viewSpace]);

  // Note: Server status changes are handled by useServerManager hook
  // which updates serverStatuses state via events. No need to re-fetch
  // server definitions on status changes - they don't change.

  // Load features for a specific server
  const loadFeaturesForServer = async (serverId: string) => {
    if (!viewSpace) return;
    
    setLoadingFeatures(prev => new Set(prev).add(serverId));
    try {
      const features = await listServerFeaturesByServer(viewSpace.id, serverId);
      setServerFeatures(prev => ({
        ...prev,
        [serverId]: features,
      }));
    } catch (e) {
      console.warn(`Failed to load features for ${serverId}:`, e);
    } finally {
      setLoadingFeatures(prev => {
        const next = new Set(prev);
        next.delete(serverId);
        return next;
      });
    }
  };

  // Toggle server expansion
  const toggleExpanded = (serverId: string) => {
    setExpandedServers(prev => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
        // Load features if not already loaded
        if (!serverFeatures[serverId]) {
          loadFeaturesForServer(serverId);
        }
      }
      return next;
    });
  };

  /**
   * Determine what action button to show based on server state:
   * - 'enable': Server is installed but not enabled
   * - 'configure': Server is enabled but missing required inputs
   * - 'connecting': Server is connecting (show spinner)
   * - 'authenticating': OAuth flow in progress (show cancel button)
   * - 'auth_required': Server needs OAuth connection (show Connect/Reconnect button)
   * - 'running': Server is connected and running
   * - 'error': Server has an error
   * - 'connected_auto': Non-OAuth server that's connected (no action buttons needed)
   * - 'disconnected': Server is enabled but has no active runtime connection
   */
  const getServerAction = (
    server: ServerViewModel
  ):
    | 'enable'
    | 'configure'
    | 'connecting'
    | 'authenticating'
    | 'auth_required'
    | 'running'
    | 'error'
    | 'connected_auto'
    | 'disconnected' => {
    if (!server.enabled) {
      return 'enable';
    }
    
    // Check if missing required inputs
    if (server.missing_required_inputs) {
      return 'configure';
    }
    
    // Get runtime status from ServerManager (event-driven)
    const runtimeStatus = getRuntimeStatus(server.id);
    
    // Use runtime status if available (more accurate, event-driven)
    if (runtimeStatus) {
      switch (runtimeStatus) {
        case 'connected':
          return server.auth?.type === 'oauth' ? 'running' : 'connected_auto';
        case 'connecting':
        case 'refreshing':
          return 'connecting';
        case 'authenticating':
          return 'authenticating';
        case 'oauth_required':
          return 'auth_required';
        case 'error':
          return 'error';
        case 'disconnected':
          return server.auth?.type === 'oauth' ? 'auth_required' : 'disconnected';
      }
    }
    
    // Use connection_status from backend as fallback
    if (server.connection_status === 'connected') {
      return server.auth?.type === 'oauth' ? 'running' : 'connected_auto';
    }
    if (server.connection_status === 'connecting') {
      return 'connecting';
    }
    if (server.connection_status === 'error') {
      return 'error';
    }
    if (server.connection_status === 'oauth_required') {
      return 'auth_required';
    }
    
    // For OAuth servers: show Connect button
    // Check both static definition and runtime oauth_connected flag
    // (some servers like Sentry declare api_key but actually use OAuth at runtime)
    if (server.auth?.type === 'oauth' || server.oauth_connected) {
      return 'auth_required';
    }

    // Enabled but no runtime connection exists yet. Let the user start/retry it.
    return 'disconnected';
  };

  /** Connected servers that show the expand/collapse chevron in the list. */
  const isServerExpandable = (server: ServerViewModel): boolean => {
    const action = getServerAction(server);
    return action === 'running' || action === 'connected_auto';
  };

  /** Expands every connected server row and loads features for any not yet fetched. */
  const expandAllServers = () => {
    const expandableIds = installedServers.filter(isServerExpandable).map((s) => s.id);
    setExpandedServers(new Set(expandableIds));
    for (const serverId of expandableIds) {
      if (!serverFeatures[serverId]) {
        loadFeaturesForServer(serverId);
      }
    }
  };

  /** Collapses every expanded server row. */
  const collapseAllServers = () => {
    setExpandedServers(new Set());
  };

  const expandableServerCount = installedServers.filter(isServerExpandable).length;
  const hasExpandedServers = expandedServers.size > 0;
  const serverCountSummary = computeServerCountSummary(installedServers, (server) =>
    getServerAction(server)
  );

  /** Installed servers matching transport, status, and search filters. */
  const filteredServers = installedServers.filter((server) =>
    serverMatchesFilters(
      server,
      searchQuery,
      serverFeatures[server.id] ?? [],
      transportFilter,
      activeStatusFilters,
      getServerAction(server) as ServerActionKey
    )
  );

  /** Toggle a Beeper-style status filter chip on or off. */
  const toggleStatusFilter = (statusKey: StatusFilterKey) => {
    setActiveStatusFilters((previous) => {
      const next = new Set(previous);
      if (next.has(statusKey)) {
        next.delete(statusKey);
      } else {
        next.add(statusKey);
      }
      return next;
    });
  };

  /** Reset transport and status filters to defaults. */
  const clearAllServerFilters = () => {
    setTransportFilter('all');
    setActiveStatusFilters(new Set());
  };

  /** Expand or collapse a connected server row; loads features on first expand. */
  const handleServerRowActivate = (server: ServerViewModel) => {
    if (!isServerExpandable(server)) {
      return;
    }
    toggleExpanded(server.id);
  };

  // Get display status for UI
  const getDisplayStatus = (server: ServerViewModel): string => {
    const action = getServerAction(server);
    const remainingSeconds = getAuthRemainingSeconds(server.id);
    
    switch (action) {
      case 'enable': return t('status.disabled');
      case 'configure': return t('status.needsConfiguration');
      case 'connecting': return t('status.connecting');
      case 'authenticating': 
        if (remainingSeconds !== undefined) {
          const minutes = Math.floor(remainingSeconds / 60);
          const seconds = remainingSeconds % 60;
          return t('status.authenticatingWithTime', { minutes, seconds });
        }
        return t('status.authenticating');
      case 'auth_required':
        return hasConnectedBefore(server.id) ? t('status.reconnectRequired') : t('status.connectRequired');
      case 'running':
        return t('status.connected');
      case 'connected_auto':
        return t('status.connected');
      case 'disconnected':
        return t('status.disconnected');
      case 'error':
        return t('status.error');
    }
  };

  // Get feature counts for a server
  const getFeatureCounts = (serverId: string) => {
    const features = serverFeatures[serverId] || [];
    return {
      tools: features.filter(f => f.feature_type === 'tool').length,
      prompts: features.filter(f => f.feature_type === 'prompt').length,
      resources: features.filter(f => f.feature_type === 'resource').length,
      total: features.length,
    };
  };

  // Handle Enable button click - uses new ServerManager v2
  const handleEnableClick = async (server: ServerViewModel) => {
    const serverInputs = server.transport.metadata?.inputs ?? [];
    // If server has required inputs that are missing, show config modal
    if (serverInputs.some((i: InputDefinition) => i.required) && server.missing_required_inputs) {
      // Initialize with existing values
      const initialValues: Record<string, string> = {};
      serverInputs.forEach((input: InputDefinition) => {
        initialValues[input.id] = server.input_values[input.id] || '';
      });
      const initialDisplayName = server.name ?? '';
      setConfigModal({
        open: true,
        server,
        inputValues: initialValues,
        enableOnSave: true,
        envOverrides: { ...(server.env_overrides ?? {}) },
        argsAppend: [...(server.args_append ?? [])],
        extraHeaders: { ...(server.extra_headers ?? {}) },
        defaultParamsJson: JSON.stringify(server.default_params ?? {}, null, 2),
        defaultParamsStrategy: server.default_params_strategy ?? 'fill',
        displayName: initialDisplayName,
        initialDisplayName,
        updatePolicy: server.update_policy ?? 'notify',
        initialUpdatePolicy: server.update_policy ?? 'notify',
        pinnedVersion: server.pinned_version ?? '',
        initialPinnedVersion: server.pinned_version ?? '',
      });
      return;
    }

    setActionLoading(`enable-${server.id}`);
    // Optimistically mark as enabled so runtime status events (Connecting/Error)
    // are reflected in the UI immediately instead of showing stale "Enable" button
    setInstalledServers(prev => prev.map(s =>
      s.id === server.id ? { ...s, enabled: true } : s
    ));
    try {
      // Use new ServerManager v2 - handles connection + OAuth in backend
      await enableServerV2(server.id);

      // Expand server to show features after connection
      setTimeout(() => {
        setExpandedServers(prev => new Set(prev).add(server.id));
        loadFeaturesForServer(server.id);
      }, 1000);
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      // Always refresh server list - the backend sets enabled=true in DB before
      // attempting connection, so we need to reflect that even on connection failure
      await loadData();
      setActionLoading(null);
    }
  };

  // Handle Disable button click - uses new ServerManager v2
  const handleDisableClick = async (server: ServerViewModel) => {
    setActionLoading(`disable-${server.id}`);
    try {
      // Use new ServerManager v2 - handles disconnect + disable in backend
      await disableServerV2(server.id);
      
      // Collapse and clear features
      setExpandedServers(prev => {
        const next = new Set(prev);
        next.delete(server.id);
        return next;
      });
      setServerFeatures(prev => {
        const next = { ...prev };
        delete next[server.id];
        return next;
      });
      
      await loadData();
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const handleConfigureClick = (server: ServerViewModel) => {
    const serverInputs = server.transport.metadata?.inputs ?? [];
    const initialValues: Record<string, string> = {};
    serverInputs.forEach((input: InputDefinition) => {
      initialValues[input.id] = server.input_values[input.id] || '';
    });
    const initialDisplayName = server.name ?? '';
    const initialUpdatePolicy = server.update_policy ?? 'notify';
    const initialPinnedVersion = server.pinned_version ?? '';
    setConfigModal({
      open: true,
      server,
      inputValues: initialValues,
      enableOnSave: false,
      envOverrides: { ...(server.env_overrides ?? {}) },
      argsAppend: [...(server.args_append ?? [])],
      extraHeaders: { ...(server.extra_headers ?? {}) },
      defaultParamsJson: JSON.stringify(server.default_params ?? {}, null, 2),
      defaultParamsStrategy: server.default_params_strategy ?? 'fill',
      displayName: initialDisplayName,
      initialDisplayName,
      updatePolicy: initialUpdatePolicy,
      initialUpdatePolicy,
      pinnedVersion: initialPinnedVersion,
      initialPinnedVersion,
    });
  };

  /**
   * Build a view model from a freshly cloned install row for the configure step.
   */
  const createViewModelFromClone = (cloned: ClonedInstalledServer): ServerViewModelWithClone | null => {
    if (!cloned.cached_definition) {
      return null;
    }

    try {
      const definition: ServerDefinition = JSON.parse(cloned.cached_definition);
      const inputValues = cloned.input_values ?? {};
      const inputs = definition.transport.metadata?.inputs ?? [];
      const missing_required_inputs = inputs.some(
        (input: InputDefinition) => input.required && !inputValues[input.id]
      );

      return {
        ...definition,
        name: resolveInstalledDisplayName(cloned, definition),
        is_installed: true,
        enabled: cloned.enabled,
        oauth_connected: cloned.oauth_connected,
        input_values: inputValues,
        connection_status: 'disconnected',
        missing_required_inputs,
        last_error: null,
        created_at: cloned.created_at,
        installation_source: cloned.source,
        cloned_from: cloned.cloned_from ?? undefined,
        env_overrides: cloned.env_overrides ?? {},
        args_append: cloned.args_append ?? [],
        extra_headers: cloned.extra_headers ?? {},
        default_params: cloned.default_params ?? {},
      };
    } catch (e) {
      console.warn('[ServersPage] Failed to parse cloned server definition:', e);
      return null;
    }
  };

  /**
   * Open the configure modal after a successful clone so the user can enter credentials.
   */
  const handleCloneComplete = async (cloned: ClonedInstalledServer) => {
    await loadData();

    const clonedViewModel = createViewModelFromClone(cloned);
    if (clonedViewModel) {
      handleConfigureClick(clonedViewModel);
      showToast(t('toast.created', { name: clonedViewModel.name }), 'success');
      return;
    }

    showToast(t('toast.accountCreated'), 'success');
  };

  const handleSaveConfig = async () => {
    if (!configModal.server) return;

    const server = configModal.server;
    const serverId = server.id;
    const shouldEnable = configModal.enableOnSave ?? false;
    const trimmedPinnedVersion = configModal.pinnedVersion.trim();

    if (configModal.updatePolicy === 'pinned') {
      if (!trimmedPinnedVersion) {
        showToast(t('toast.enterPinnedVersion'), 'error');
        return;
      }
      if (!isValidSemver(trimmedPinnedVersion)) {
        showToast(t('toast.invalidSemver'), 'error');
        return;
      }
    }

    setActionLoading(`config-${serverId}`);
    try {
      const { saveServerInputs } = await import('@/lib/api/registry');

      const trimmedDisplayName = configModal.displayName.trim();
      const trimmedInitial = configModal.initialDisplayName.trim();
      // Only send a value when the user actually edited the field; otherwise pass
      // undefined so the backend leaves the existing override untouched.
      const displayNameOverride =
        trimmedDisplayName === trimmedInitial ? undefined : trimmedDisplayName;

      const updatePolicyChanged = configModal.updatePolicy !== configModal.initialUpdatePolicy;
      const pinnedVersionChanged =
        trimmedPinnedVersion !== configModal.initialPinnedVersion.trim();
      const updatePolicy =
        updatePolicyChanged || pinnedVersionChanged ? configModal.updatePolicy : undefined;
      const pinnedVersion =
        configModal.updatePolicy === 'pinned'
          ? trimmedPinnedVersion
          : updatePolicyChanged
            ? ''
            : undefined;

      let defaultParams: Record<string, unknown> | undefined;
      try {
        const parsed = JSON.parse(configModal.defaultParamsJson.trim() || '{}');
        if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
          defaultParams = parsed as Record<string, unknown>;
        }
      } catch {
        // ignore parse errors — backend will keep the existing value
      }

      await saveServerInputs(
        serverId,
        configModal.inputValues,
        viewSpace?.id ?? '',
        configModal.envOverrides,
        configModal.argsAppend,
        configModal.extraHeaders,
        defaultParams,
        configModal.defaultParamsStrategy,
        displayNameOverride,
        updatePolicy,
        pinnedVersion,
      );

      setConfigModal({
        open: false,
        server: null,
        inputValues: {},
        envOverrides: {},
        argsAppend: [],
        extraHeaders: {},
        defaultParamsJson: '{}',
        defaultParamsStrategy: 'fill',
        displayName: '',
        initialDisplayName: '',
        updatePolicy: 'notify',
        initialUpdatePolicy: 'notify',
        pinnedVersion: '',
        initialPinnedVersion: '',
      });
      
      // Only enable if requested (from Enable flow)
      if (shouldEnable && !server.enabled) {
        // Optimistically mark as enabled so runtime status events are reflected
        setInstalledServers(prev => prev.map(s =>
          s.id === serverId ? { ...s, enabled: true, missing_required_inputs: false } : s
        ));
        // Use new ServerManager v2 to enable and connect
        await enableServerV2(serverId);

        setTimeout(() => {
          setExpandedServers(prev => new Set(prev).add(serverId));
          loadFeaturesForServer(serverId);
        }, 1000);
      } else if (server.enabled) {
        // If already enabled, trigger reconnect with new config
        await retryConnectionV2(serverId);
      }

      showToast(t('toast.configSaved'), 'success');
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      // Always refresh server list to reflect DB state (enabled, config changes)
      // even if connection failed
      await loadData();
      setActionLoading(null);
    }
  };
  
  // Handle cancel on config modal - if from Enable flow, mark as pending_config
  const handleCancelConfig = async () => {
    if (configModal.enableOnSave && configModal.server && !configModal.server.enabled) {
      // User cancelled during Enable flow with missing inputs
      // Set the server to pending_config state by enabling but not connecting
      // Actually, we just close the modal - the UI already shows Configure button for missing inputs
    }
    setConfigModal({
      open: false,
      server: null,
      inputValues: {},
      envOverrides: {},
      argsAppend: [],
      extraHeaders: {},
      defaultParamsJson: '{}',
      defaultParamsStrategy: 'fill',
      displayName: '',
      initialDisplayName: '',
      updatePolicy: 'notify',
      initialUpdatePolicy: 'notify',
      pinnedVersion: '',
      initialPinnedVersion: '',
    });
  };

  /**
   * Pin the server's current package version and switch policy to Pinned.
   */
  const handleLockToCurrentVersion = async (server: ServerViewModel) => {
    if (!viewSpace) {
      return;
    }

    setActionLoading(`lock-version-${server.id}`);
    try {
      const { saveServerInputs } = await import('@/lib/api/registry');

      let version =
        resolveCurrentPackageVersion({
          pinnedVersion: server.pinned_version,
          transportCommand:
            server.transport.type === 'stdio' ? server.transport.command : undefined,
          transportArgs:
            server.transport.type === 'stdio' ? server.transport.args : undefined,
          installedVersion: server.current_version,
        }) ?? server.latest_available_version;

      if (!version) {
        const probe = await checkServerVersion(viewSpace.id, server.id);
        version = probe.currentVersion ?? probe.latestVersion;
        setInstalledServers((current) =>
          current.map((entry) => {
            if (entry.id !== server.id) {
              return entry;
            }
            return {
              ...entry,
              latest_available_version: probe.latestVersion ?? entry.latest_available_version,
              version_checked_at: probe.checkedAt,
            };
          })
        );
      }

      if (!version || !isValidSemver(version)) {
        showToast(t('toast.cannotPin'), 'error');
        return;
      }

      await saveServerInputs(
        server.id,
        server.input_values,
        viewSpace.id,
        server.env_overrides,
        server.args_append,
        server.extra_headers,
        server.default_params,
        undefined,
        'pinned',
        version
      );

      if (server.enabled) {
        await retryConnectionV2(server.id);
      }

      showToast(t('toast.lockedTo', { version }), 'success');
      await loadData();
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  // Cancel OAuth flow - uses new ServerManager v2
  const handleCancelOAuth = async (serverId: string) => {
    try {
      await cancelAuthV2(serverId);
    } catch (e) {
      console.warn('[ServersPage] Cancel OAuth failed:', e);
    }
  };
  
  // Start OAuth flow (Connect button) - uses new ServerManager v2
  const handleConnect = async (server: ServerViewModel) => {
    setActionLoading(`connect-${server.id}`);
    try {
      await startAuthV2(server.id);
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };
  
  // Retry connection - uses new ServerManager v2
  const handleRetry = async (server: ServerViewModel) => {
    setActionLoading(`retry-${server.id}`);
    try {
      await retryConnectionV2(server.id);
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  /**
   * Apply latest package and reconnect (notify/auto npx/uvx servers).
   */
  const handleUpdateNow = async (server: ServerViewModel) => {
    if (!viewSpace) {
      return;
    }

    setActionLoading(`update-${server.id}`);
    try {
      const { updateServerPackage } = await import('@/lib/api/serverManager');
      await updateServerPackage(viewSpace.id, server.id);
      showToast(t('toast.updating', { name: server.name }), 'info');
      await loadData();
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  /**
   * Run an immediate npm/uv version probe for one server.
   */
  const handleCheckForUpdate = async (server: ServerViewModel) => {
    if (!viewSpace) {
      return;
    }

    setActionLoading(`check-update-${server.id}`);
    try {
      const result = await checkServerVersion(viewSpace.id, server.id);
      setInstalledServers((current) =>
        current.map((entry) => {
          if (entry.id !== server.id) {
            return entry;
          }
          return {
            ...entry,
            latest_available_version: result.latestVersion,
            version_checked_at: result.checkedAt,
          };
        })
      );

      if (result.updateAvailable && result.latestVersion) {
        showToast(t('toast.updateAvailable', { version: result.latestVersion }), 'info');
      } else {
        showToast(t('toast.upToDate'), 'success');
      }
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const performUninstall = async (serverIds: string[]) => {
    const { uninstallServer } = await import('@/lib/api/registry');
    const { disconnectServer } = await import('@/lib/api/gateway');

    if (gatewayRunning && viewSpace) {
      for (const serverId of serverIds) {
        const target = installedServers.find((entry) => entry.id === serverId);
        if (!target?.enabled) {
          continue;
        }

        try {
          await disconnectServer(serverId, viewSpace.id);
        } catch (error) {
          console.warn(`[ServersPage] Failed to disconnect server from gateway:`, error);
        }
      }
    }

    for (const serverId of serverIds) {
      await uninstallServer(serverId, viewSpace?.id ?? '');
    }

    await loadData();
  };

  const handleUninstall = async (server: ServerViewModelWithClone) => {
    if (!viewSpace) {
      return;
    }

    if (!server.cloned_from) {
      try {
        const dependents = await listCloneDependents(viewSpace.id, server.id);
        if (dependents.length > 0) {
          setUninstallClonesDialog({ server, dependents });
          return;
        }
      } catch (error) {
        showToast(String(error), 'error');
        return;
      }
    }

    const { getUninstallLabel } = await import('@/components/source-badge.helpers');
    const actionLabel = getUninstallLabel(tCommon, server.installation_source);

    setActionLoading(`uninstall-${server.id}`);
    try {
      await performUninstall([server.id]);
      showToast(`${server.name} ${actionLabel.toLowerCase()}ed`, 'success');
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const handleUninstallSourceOnly = async () => {
    if (!uninstallClonesDialog) {
      return;
    }

    const { server } = uninstallClonesDialog;
    const { getUninstallLabel } = await import('@/components/source-badge.helpers');
    const actionLabel = getUninstallLabel(tCommon, server.installation_source);

    setUninstallClonesDialog(null);
    setActionLoading(`uninstall-${server.id}`);
    try {
      await performUninstall([server.id]);
      showToast(`${server.name} ${actionLabel.toLowerCase()}ed`, 'success');
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const handleUninstallAllWithClones = async () => {
    if (!uninstallClonesDialog) {
      return;
    }

    const { server, dependents } = uninstallClonesDialog;
    const serverIds = [...dependents.map((dependent) => dependent.server_id), server.id];

    setUninstallClonesDialog(null);
    setActionLoading(`uninstall-${server.id}`);
    try {
      await performUninstall(serverIds);
      showToast(
        t('toast.uninstalledWithClones', { name: server.name, count: dependents.length }),
        'success'
      );
    } catch (error) {
      showToast(String(error), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const handleStartGateway = async () => {
    try {
      const outcome = await gatewayControl.start();
      if (outcome.status === 'cancelled') return;
      setGatewayRunning(true);
      setGatewayUrl(outcome.url);
      if (outcome.fellBackToDynamic) {
        showToast(
          t('toast.gatewayPortFallback', { port: outcome.port }),
          'info'
        );
      }

      // Auto-connect all enabled servers
      try {
        const { connectAllEnabledServers } = await import('@/lib/api/gateway');
        await connectAllEnabledServers();
      } catch (e) {
        console.warn('[ServersPage] Failed to auto-connect servers:', e);
      }

      await loadData();
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  // Disconnect a server (with optional logout) - old gateway method
  const handleDisconnect = async (server: ServerViewModel, logout: boolean = false) => {
    if (!viewSpace) return;
    
    setActionLoading(`disconnect-${server.id}`);
    try {
      const { disconnectServer } = await import('@/lib/api/gateway');
      await disconnectServer(server.id, viewSpace.id, logout);
      await loadData();
      // Clear features when disconnecting
      setServerFeatures(prev => {
        const next = { ...prev };
        delete next[server.id];
        return next;
      });
      if (logout) {
        showToast(t('toast.disconnectedLoggedOut', { name: server.name }), 'info');
      }
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };


  // Refresh server - Quick reconnect with EXISTING credentials
  // If succeeds → connected, if fails → shows Connect button
  const handleRefresh = async (server: ServerViewModel) => {
    setActionLoading(`refresh-${server.id}`);
    try {
      await retryConnectionV2(server.id);
      await loadData();
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  // Reconnect server - Logout + auto-start OAuth (for OAuth servers)
  // For non-OAuth servers, just does a fresh connection
  const handleReconnect = async (server: ServerViewModel) => {
    setActionLoading(`reconnect-${server.id}`);
    try {
      // OAuth is detected at runtime - check if server has oauth_connected or auth type
      const isOAuthServer = server.auth?.type === 'oauth' || server.oauth_connected;
      
      if (isOAuthServer) {
        // Clear tokens first
        const { logoutServer } = await import('@/lib/api/serverManager');
        await logoutServer(viewSpace?.id ?? '', server.id);
        await loadData();
        // Auto-start OAuth flow (opens browser)
        await startAuthV2(server.id);
      } else {
        // Non-OAuth: just retry
        await retryConnectionV2(server.id);
        await loadData();
      }
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  if (isLoading && installedServers.length === 0) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin rounded-full h-8 w-8 border-2 border-[rgb(var(--primary))] border-t-transparent" />
      </div>
    );
  }

  return (
    <div data-testid="servers-page">
      {gatewayControl.ConfirmDialogElement}
      {uninstallClonesDialog && (
        <UninstallSourceWithClonesDialog
          open
          sourceName={uninstallClonesDialog.server.name}
          dependents={uninstallClonesDialog.dependents}
          onCancel={() => setUninstallClonesDialog(null)}
          onUninstallSourceOnly={handleUninstallSourceOnly}
          onUninstallAll={handleUninstallAllWithClones}
        />
      )}
      {/* Toolbar — stays visible while the server list scrolls in <main> */}
      <div
        className="sticky -top-6 z-20 -mx-6 mb-4 space-y-4 border-b border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] px-6 pt-6 pb-4"
        data-testid="servers-page-toolbar"
      >
        <div className="flex items-center justify-between gap-4">
          <div className="flex-shrink min-w-0">
            <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
              <h1 className="text-2xl font-bold" data-testid="servers-title">
                {t('title')}
              </h1>
              <ServersCountSummary summary={serverCountSummary} />
            </div>
            <p className="text-sm text-[rgb(var(--muted))]">{t('subtitle')}</p>
          </div>

          <div
            className={`flex items-center gap-2 px-3 py-2 rounded-lg border text-sm flex-shrink-0 ${
              gatewayRunning
                ? 'bg-[rgb(var(--success))]/10 border-[rgb(var(--success))]/30'
                : 'bg-[rgb(var(--warning))]/10 border-[rgb(var(--warning))]/30'
            }`}
            data-testid="gateway-status-chip"
            data-state={gatewayRunning ? 'running' : 'stopped'}
          >
            <span
              className={`h-2.5 w-2.5 rounded-full flex-shrink-0 ${
                gatewayRunning ? 'bg-[rgb(var(--success))] animate-pulse' : 'bg-[rgb(var(--warning))]'
              }`}
            />
            <span className="font-medium whitespace-nowrap">
              {gatewayRunning ? t('gateway.running') : t('gateway.stopped')}
            </span>
            {gatewayRunning && gatewayUrl && (
              <code className="text-xs bg-[rgb(var(--surface-elevated))] px-2 py-0.5 rounded text-[rgb(var(--primary))] truncate max-w-[200px]">
                {gatewayUrl}
              </code>
            )}
            {!gatewayRunning && viewSpace && (
              <button
                type="button"
                onClick={handleStartGateway}
                className="px-2.5 py-1 text-xs font-medium bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] rounded-md hover:bg-[rgb(var(--primary-hover))] transition-colors whitespace-nowrap"
              >
                {t('gateway.start')}
              </button>
            )}
          </div>

          {viewSpace && (
            <div className="flex items-center gap-2 flex-shrink-0">
              {installedServers.length > 0 && (
                <>
                <Button
                  variant="secondary"
                  size="md"
                  type="button"
                  onClick={expandAllServers}
                  disabled={expandableServerCount === 0}
                  title={t('toolbar.expandAllTitle')}
                  data-testid="expand-all-servers"
                >
                  <UnfoldVertical className="h-4 w-4 text-[rgb(var(--muted))]" />
                  {t('toolbar.expandAll')}
                </Button>
                <Button
                  variant="secondary"
                  size="md"
                  type="button"
                  onClick={collapseAllServers}
                  disabled={!hasExpandedServers}
                  title={t('toolbar.collapseAllTitle')}
                  data-testid="collapse-all-servers"
                >
                  <FoldVertical className="h-4 w-4 text-[rgb(var(--muted))]" />
                  {t('toolbar.collapseAll')}
                </Button>
                </>
              )}
              <AddServerMenu
                onDiscover={() => navigateTo('registry')}
                onCustom={() => setEditConfigSpace({ id: viewSpace.id, name: viewSpace.name })}
              />
            </div>
          )}
        </div>

        {viewSpace && installedServers.length > 0 && (
          <div className="flex items-center gap-2 w-full">
            <SearchField
              className="flex-1 min-w-0"
              placeholder={t('toolbar.searchPlaceholder')}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onClear={() => setSearchQuery('')}
              data-testid="servers-search"
            />
            <ServersFiltersPopover
              transportFilter={transportFilter}
              onTransportFilterChange={setTransportFilter}
              activeStatusFilters={activeStatusFilters}
              onToggleStatusFilter={toggleStatusFilter}
              onClearStatusFilters={() => setActiveStatusFilters(new Set())}
              onClearAllFilters={clearAllServerFilters}
            />
          </div>
        )}
      </div>

      {/* Server List */}
      {installedServers.length === 0 ? (
        <div className="text-center py-12 text-[rgb(var(--muted))]" data-testid="servers-empty-state">
          <div className="text-5xl mb-4">📦</div>
          <p className="text-lg mb-2">{t('empty.noneInstalled')}</p>
          <p className="text-sm max-w-md mx-auto mb-4">
            {t('empty.noneInstalledDesc')}
          </p>
          {viewSpace && (
            <div className="flex justify-center">
              <AddServerMenu
                onDiscover={() => navigateTo('registry')}
                onCustom={() => setEditConfigSpace({ id: viewSpace.id, name: viewSpace.name })}
              />
            </div>
          )}
        </div>
      ) : filteredServers.length === 0 ? (
        <div className="text-center py-12 text-[rgb(var(--muted))]">
          <Search className="h-12 w-12 mx-auto mb-4 opacity-40" />
          <p className="text-lg mb-2">{t('empty.noMatch')}</p>
          <p className="text-sm">{t('empty.noMatchDesc')}</p>
        </div>
      ) : (
        <div className="space-y-3">
          {filteredServers.map((server) => {
            const serverAction = getServerAction(server);
            const displayStatus = getDisplayStatus(server);
            const enableLoading = actionLoading === `enable-${server.id}`;
            const disableLoading = actionLoading === `disable-${server.id}`;
            const configLoading = actionLoading === `config-${server.id}`;
            const connectLoading = actionLoading === `connect-${server.id}`;
            const retryLoading = actionLoading === `retry-${server.id}`;
            const isExpanded = expandedServers.has(server.id);
            const isLoadingServerFeatures = loadingFeatures.has(server.id);
            const features = serverFeatures[server.id] || [];
            const counts = getFeatureCounts(server.id);
            const isConnected = serverAction === 'running' || serverAction === 'connected_auto';
            const isAuthenticating = serverAction === 'authenticating';
            const runtimeMessage = serverStatuses[server.id]?.message;

            return (
              <div
                key={server.id}
                className="bg-[rgb(var(--card))] border border-[rgb(var(--border-subtle))] rounded-xl shadow-sm transition-all"
                data-testid={`installed-server-${server.id}`}
              >
                {/* Server Header */}
                <div className="p-4">
                  <div className="flex items-center justify-between gap-4">
                    <div
                      role={isServerExpandable(server) ? 'button' : undefined}
                      tabIndex={isServerExpandable(server) ? 0 : undefined}
                      onClick={() => handleServerRowActivate(server)}
                      onKeyDown={(event) => {
                        if (!isServerExpandable(server)) {
                          return;
                        }
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault();
                          handleServerRowActivate(server);
                        }
                      }}
                      className={`flex items-center gap-4 flex-1 min-w-0 ${
                        !server.enabled ? 'opacity-60' : ''
                      } ${isServerExpandable(server) ? 'cursor-pointer rounded-lg hover:bg-[rgb(var(--surface-hover))]/60 -m-2 p-2 transition-colors' : ''}`}
                      data-testid={`server-row-${server.id}`}
                    >
                      {isServerExpandable(server) && (
                        <span className="p-1" data-testid={`expand-server-${server.id}`}>
                          {isExpanded ? (
                            <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                          ) : (
                            <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                          )}
                        </span>
                      )}
                      
                      <div className="text-3xl flex items-center justify-center">
                        {server.icon?.startsWith('http') ? (
                          <img src={server.icon} alt="" className="w-8 h-8 object-contain" onError={(e) => { e.currentTarget.style.display = 'none'; e.currentTarget.parentElement!.append(document.createTextNode('📦')); }} />
                        ) : (
                          server.icon || '📦'
                        )}
                      </div>
                      <div>
                        <div className="font-medium">{server.name}</div>
                        <div className="text-sm text-[rgb(var(--muted))] max-w-md truncate">
                          {server.description}
                        </div>
                        <div className="flex items-center gap-2 mt-2 flex-wrap">
                          {/* State Badge */}
                          <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-xs font-medium ${
                            serverAction === 'running' || serverAction === 'connected_auto' 
                              ? 'bg-[rgb(var(--success))]/15 text-[rgb(var(--success))]' :
                            serverAction === 'error' 
                              ? 'bg-[rgb(var(--error))]/15 text-[rgb(var(--error))]' :
                            serverAction === 'configure' || serverAction === 'auth_required'
                              ? 'bg-[rgb(var(--warning))]/15 text-[rgb(var(--warning))]' :
                            serverAction === 'connecting' || serverAction === 'authenticating'
                              ? 'bg-blue-500/15 text-blue-600 dark:text-blue-400' :
                            'bg-[rgb(var(--muted))]/10 text-[rgb(var(--muted))]'
                          }`}>
                            {serverAction === 'connecting' || serverAction === 'authenticating' ? (
                              <Loader2 className="h-3 w-3 animate-spin" />
                            ) : (
                              <span className={`h-1.5 w-1.5 rounded-full ${
                                serverAction === 'running' || serverAction === 'connected_auto' ? 'bg-[rgb(var(--success))]' :
                                serverAction === 'error' ? 'bg-[rgb(var(--error))]' :
                                serverAction === 'configure' || serverAction === 'auth_required' ? 'bg-[rgb(var(--warning))]' :
                                'bg-[rgb(var(--muted))]'
                              }`} />
                            )}
                            {displayStatus}
                          </span>
                          
                          {/* Feature counts for connected servers */}
                          {isConnected && counts.total > 0 && (
                            <>
                              {counts.tools > 0 && (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs bg-purple-500/15 text-purple-600 dark:text-purple-400">
                                  <Wrench className="h-3 w-3" />
                                  {t('features.tools', { count: counts.tools })}
                                </span>
                              )}
                              {counts.prompts > 0 && (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs bg-blue-500/15 text-blue-600 dark:text-blue-400">
                                  <MessageSquare className="h-3 w-3" />
                                  {t('features.prompts', { count: counts.prompts })}
                                </span>
                              )}
                              {counts.resources > 0 && (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs bg-green-500/15 text-green-600 dark:text-green-400">
                                  <FileText className="h-3 w-3" />
                                  {t('features.resources', { count: counts.resources })}
                                </span>
                              )}
                            </>
                          )}
                          
                          {/* Auth Type Badge */}
                          {server.auth && server.auth.type !== 'none' && (
                            <span className="text-xs text-[rgb(var(--muted))] px-2 py-0.5 bg-[rgb(var(--surface-hover))] rounded-md">
                              {server.auth.type === 'oauth' ? t('auth.oauth') : 
                               server.auth.type === 'api_key' ? t('auth.apiKey') :
                               server.auth.type === 'optional_api_key' ? t('auth.apiKeyOptional') :
                               t('auth.required')}
                            </span>
                          )}
                          
                          <span className="text-xs text-[rgb(var(--muted))]">{server.transport.type}</span>
                          
                          {/* Installation Source Badge */}
                          <SourceBadge
                            source={server.installation_source}
                            clonedFrom={server.cloned_from}
                          />
                        </div>
                        
                        {/* Show runtime message inline (from ServerManager events) */}
                        {isAuthenticating && (
                          <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400">
                            <Clock className="h-3 w-3" />
                            <span>
                              {runtimeMessage || t('auth.waitingBrowser')}
                            </span>
                          </div>
                        )}
                        
                        {/* Show error indicator if in error state */}
                        {serverAction === 'error' && (
                          <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg text-xs bg-[rgb(var(--error))]/10 text-[rgb(var(--error))]">
                            <span className="font-medium">{t('connection.error')}</span>
                            <span className="text-[rgb(var(--muted))]">·</span>
                            <button
                              type="button"
                              onClick={(event) => {
                                event.stopPropagation();
                                setLogViewerServer({ id: server.id, name: server.name });
                              }}
                              className="text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] underline cursor-pointer transition-colors"
                            >
                              {t('connection.viewLogs')}
                            </button>
                          </div>
                        )}
                      </div>
                    </div>

                    {/* Actions - horizontal row with primary and secondary actions */}
                    <div
                      className="flex items-center gap-2 flex-shrink-0"
                      onClick={(event) => event.stopPropagation()}
                    >
                      {(serverAction === 'enable' ||
                        (server.enabled &&
                          (serverAction === 'running' ||
                            serverAction === 'connected_auto' ||
                            serverAction === 'error'))) && (
                        <ServerEnabledToggle
                          serverId={server.id}
                          enabled={server.enabled}
                          isLoading={enableLoading || disableLoading}
                          onToggle={(checked) => {
                            if (checked) {
                              handleEnableClick(server);
                            } else {
                              handleDisableClick(server);
                            }
                          }}
                        />
                      )}

                      {serverAction === 'configure' && (
                        <button
                          onClick={() => handleConfigureClick(server)}
                          disabled={configLoading}
                          className="px-4 py-2 text-sm font-medium rounded-lg bg-[rgb(var(--warning))] text-white hover:bg-[rgb(var(--warning))]/80 shadow-sm transition-colors disabled:opacity-50"
                        >
                          {configLoading ? t('actions.saving') : t('actions.configure')}
                        </button>
                      )}

                      {/* Connecting state - show spinner */}
                      {serverAction === 'connecting' && (
                        <button
                          disabled
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--surface-elevated))] text-[rgb(var(--muted))] cursor-not-allowed flex items-center gap-2"
                        >
                          <Loader2 className="h-4 w-4 animate-spin" />
                          {t('actions.connecting')}
                        </button>
                      )}
                      
                      {/* Authenticating state - show cancel button */}
                      {serverAction === 'authenticating' && (
                        <>
                          <button
                            disabled
                            className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--warning))] text-white cursor-not-allowed flex items-center gap-2"
                          >
                            <Loader2 className="h-4 w-4 animate-spin" />
                            {t('actions.authenticating')}
                          </button>
                          <button
                            onClick={() => handleCancelOAuth(server.id)}
                            className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                          >
                            {t('actions.cancel')}
                          </button>
                        </>
                      )}
                      
                      {/* Auth Required state - show Connect/Reconnect button */}
                      {serverAction === 'auth_required' && gatewayRunning && (
                        <button
                          onClick={() => handleConnect(server)}
                          disabled={connectLoading}
                          className="px-4 py-2 text-sm font-medium rounded-lg bg-[rgb(var(--success))] text-white hover:bg-[rgb(var(--success))]/80 shadow-sm transition-colors disabled:opacity-50"
                        >
                          {connectLoading ? t('actions.connecting') : hasConnectedBefore(server.id) ? t('actions.reconnect') : t('actions.connect')}
                        </button>
                      )}

                      {/* Running state - show Disconnect button */}
                      {serverAction === 'running' && (
                        <button
                          onClick={() => handleDisconnect(server, false)}
                          disabled={actionLoading === `disconnect-${server.id}`}
                          className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-50"
                          title={t('actions.disconnectTitle')}
                        >
                          {actionLoading === `disconnect-${server.id}` ? '...' : t('actions.disconnect')}
                        </button>
                      )}

                      {serverAction === 'disconnected' && gatewayRunning && (
                        <button
                          onClick={() => handleRetry(server)}
                          disabled={retryLoading}
                          className="rounded-lg bg-[rgb(var(--success))] px-4 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-[rgb(var(--success))]/80 disabled:opacity-50"
                        >
                          {retryLoading ? 'Connecting...' : 'Connect'}
                        </button>
                      )}

                      {serverAction === 'error' && gatewayRunning && (
                        <button
                          onClick={() => handleRetry(server)}
                          disabled={retryLoading}
                          className="px-4 py-2 text-sm font-medium rounded-lg bg-[rgb(var(--error))] text-white hover:bg-[rgb(var(--error))]/80 shadow-sm transition-colors disabled:opacity-50"
                        >
                          {retryLoading ? t('actions.retrying') : hasConnectedBefore(server.id) ? t('actions.reconnect') : t('actions.retry')}
                        </button>
                      )}

                      {/* Disable button - shown when an enabled server is connected,
                          running, or sitting disconnected (so it can still be turned off
                          without first reconnecting) */}
                      {server.enabled &&
                        (serverAction === 'running' ||
                          serverAction === 'connected_auto' ||
                          serverAction === 'disconnected') && (
                          <button
                            onClick={() => handleDisableClick(server)}
                            disabled={disableLoading}
                            className="rounded-lg border border-[rgb(var(--border))] px-4 py-2 text-sm text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))] disabled:opacity-50"
                            data-testid={`disable-server-${server.id}`}
                          >
                            {disableLoading ? '...' : t('actions.disable')}
                          </button>
                        )}

                      {/* Overflow menu with secondary actions */}
                      <ServerActionMenu
                        serverId={server.id}
                        serverName={server.name}
                        hasInputs={(server.transport.metadata?.inputs ?? []).length > 0}
                        isOAuth={
                          // OAuth is detected at runtime, not always in definition
                          server.auth?.type === 'oauth' || 
                          server.oauth_connected || 
                          serverAction === 'auth_required'
                        }
                        isEnabled={server.enabled}
                        isConnected={serverAction === 'running' || serverAction === 'connected_auto'}
                        isPackageManaged={
                          server.transport.type === 'stdio' &&
                          isPackageManagedTransport(server.transport.command)
                        }
                        updatePolicy={server.update_policy ?? 'notify'}
                        hasUpdateAvailable={
                          server.transport.type === 'stdio' &&
                          shouldShowPackageUpdate({
                            updatePolicy: server.update_policy ?? 'notify',
                            latestVersion: server.latest_available_version,
                            currentVersion: resolveCurrentPackageVersion({
                              pinnedVersion: server.pinned_version,
                              transportCommand: server.transport.command,
                              transportArgs: server.transport.args,
                              installedVersion: server.current_version,
                            }),
                            transportCommand: server.transport.command,
                            transportArgs: server.transport.args,
                          })
                        }
                        latestVersion={server.latest_available_version}
                        canCloneAccount={canCloneServer(server)}
                        onConfigure={() => handleConfigureClick(server)}
                        onRefresh={() => handleRefresh(server)}
                        onReconnect={() => handleReconnect(server)}
                        onUpdateNow={() => handleUpdateNow(server)}
                        onCheckForUpdate={() => handleCheckForUpdate(server)}
                        onLockToCurrentVersion={() => handleLockToCurrentVersion(server)}
                        onViewLogs={() => setLogViewerServer({ id: server.id, name: server.name })}
                        onViewDefinition={() => setDefinitionServer({ id: server.id, name: server.name })}
                        onCloneAccount={() => setCloneModalServer(server)}
                        onUninstall={() => handleUninstall(server)}
                      />
                    </div>
                  </div>
                </div>

                {/* Expanded Features Section */}
                {isExpanded && isConnected && (
                  <div className="border-t border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-dim))]">
                    {isLoadingServerFeatures ? (
                      <div className="flex items-center justify-center py-8">
                        <Loader2 className="h-6 w-6 animate-spin text-[rgb(var(--primary))]" />
                        <span className="ml-2 text-sm text-[rgb(var(--muted))]">{t('features.loading')}</span>
                      </div>
                    ) : features.length === 0 ? (
                      <div className="text-center py-8 text-[rgb(var(--muted))]">
                        <p className="text-sm">{t('features.noneDiscovered')}</p>
                        <p className="text-xs mt-1">{t('features.noneDiscoveredHint')}</p>
                        <button
                          onClick={() => loadFeaturesForServer(server.id)}
                          className="mt-3 px-3 py-1 text-xs rounded bg-[rgb(var(--surface-hover))] hover:bg-[rgb(var(--surface-active))] transition-colors"
                        >
                          {t('actions.refresh')}
                        </button>
                      </div>
                    ) : (
                      <div className="p-4 space-y-4">
                        {/* Tools */}
                        {features.filter(f => f.feature_type === 'tool').length > 0 && (
                          <div>
                            <h4 className="text-sm font-medium flex items-center gap-2 mb-2">
                              <Wrench className="h-4 w-4 text-purple-500" />
                              {t('features.toolsHeading', { count: features.filter(f => f.feature_type === 'tool').length })}
                            </h4>
                            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2">
                              {features.filter(f => f.feature_type === 'tool').map(feature => (
                                <div
                                  key={feature.id}
                                  className="p-3 bg-[rgb(var(--card))] rounded-lg border border-[rgb(var(--border-subtle))]"
                                >
                                  <div className="font-medium text-sm">
                                    {feature.display_name || feature.feature_name}
                                  </div>
                                  {feature.description && (
                                    <p className="text-xs text-[rgb(var(--muted))] mt-1 line-clamp-2">
                                      {feature.description}
                                    </p>
                                  )}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}

                        {/* Prompts */}
                        {features.filter(f => f.feature_type === 'prompt').length > 0 && (
                          <div>
                            <h4 className="text-sm font-medium flex items-center gap-2 mb-2">
                              <MessageSquare className="h-4 w-4 text-blue-500" />
                              {t('features.promptsHeading', { count: features.filter(f => f.feature_type === 'prompt').length })}
                            </h4>
                            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2">
                              {features.filter(f => f.feature_type === 'prompt').map(feature => (
                                <div
                                  key={feature.id}
                                  className="p-3 bg-[rgb(var(--card))] rounded-lg border border-[rgb(var(--border-subtle))]"
                                >
                                  <div className="font-medium text-sm">
                                    {feature.display_name || feature.feature_name}
                                  </div>
                                  {feature.description && (
                                    <p className="text-xs text-[rgb(var(--muted))] mt-1 line-clamp-2">
                                      {feature.description}
                                    </p>
                                  )}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}

                        {/* Resources */}
                        {features.filter(f => f.feature_type === 'resource').length > 0 && (
                          <div>
                            <h4 className="text-sm font-medium flex items-center gap-2 mb-2">
                              <FileText className="h-4 w-4 text-green-500" />
                              {t('features.resourcesHeading', { count: features.filter(f => f.feature_type === 'resource').length })}
                            </h4>
                            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2">
                              {features.filter(f => f.feature_type === 'resource').map(feature => (
                                <div
                                  key={feature.id}
                                  className="p-3 bg-[rgb(var(--card))] rounded-lg border border-[rgb(var(--border-subtle))]"
                                >
                                  <div className="font-medium text-sm">
                                    {feature.display_name || feature.feature_name}
                                  </div>
                                  {feature.description && (
                                    <p className="text-xs text-[rgb(var(--muted))] mt-1 line-clamp-2">
                                      {feature.description}
                                    </p>
                                  )}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Clone Account Modal */}
      {cloneModalServer && viewSpace && (
        <CloneAccountModal
          open={!!cloneModalServer}
          spaceId={viewSpace.id}
          sourceServer={cloneModalServer}
          onClose={() => setCloneModalServer(null)}
          onCloned={handleCloneComplete}
        />
      )}

      {/* Configuration Modal */}
      {configModal.open && configModal.server && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4" data-testid="config-modal-overlay">
          <div className="dropdown-menu w-full max-w-md p-6 animate-in fade-in scale-in duration-150 max-h-[80vh] overflow-y-auto" data-testid="config-modal">
            <h3 className="text-lg font-semibold text-[rgb(var(--foreground))] mb-2" data-testid="config-modal-title">
              {t('configModal.title', { name: configModal.server.name })}
            </h3>
            <p className="text-sm text-[rgb(var(--muted))] mb-4">
              {(configModal.server.auth && 'instructions' in configModal.server.auth ? configModal.server.auth.instructions : null) || t('configModal.defaultInstructions')}
            </p>

            <div className="space-y-4">
              <div>
                <label
                  htmlFor="config-display-name"
                  className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
                >
                  {t('configModal.displayName')}
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mb-2">
                  {t('configModal.displayNameDesc')}
                </p>
                <input
                  id="config-display-name"
                  type="text"
                  value={configModal.displayName}
                  onChange={(e) =>
                    setConfigModal({ ...configModal, displayName: e.target.value })
                  }
                  placeholder={configModal.server.name}
                  className="input w-full"
                  data-testid="config-display-name"
                />
              </div>

              {(configModal.server.transport.metadata?.inputs ?? []).map((input: InputDefinition) => {
                const obtainUrl = input.obtain_url || input.obtain?.url;
                const obtainInstructions = input.obtain_instructions || input.obtain?.instructions;
                const inputType = input.type || 'text';
                const currentValue = configModal.inputValues[input.id] ?? '';
                
                const handleChange = (value: string) => {
                  setConfigModal({
                    ...configModal,
                    inputValues: { ...configModal.inputValues, [input.id]: value }
                  });
                };
                
                const renderInput = () => {
                  switch (inputType) {
                    case 'boolean':
                      return (
                        <label className="flex items-center gap-2 cursor-pointer">
                          <input
                            type="checkbox"
                            checked={currentValue === 'true'}
                            onChange={(e) => handleChange(e.target.checked ? 'true' : 'false')}
                            className="w-4 h-4 rounded border-[rgb(var(--border))] text-[rgb(var(--primary))] focus:ring-[rgb(var(--primary))]"
                          />
                          <span className="text-sm text-[rgb(var(--muted))]">
                            {input.placeholder || t('configModal.enable')}
                          </span>
                        </label>
                      );
                    case 'number':
                      return (
                        <input
                          type="number"
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          placeholder={input.placeholder || '0'}
                          className="input w-full"
                        />
                      );
                    case 'url':
                      return (
                        <input
                          type="url"
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          placeholder={input.placeholder || 'https://...'}
                          className="input w-full"
                        />
                      );
                    case 'select':
                      return (
                        <select
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          className="input w-full"
                          data-testid={`config-input-${input.id}`}
                        >
                          <option value="">{input.placeholder || t('configModal.selectOption', { label: input.label.toLowerCase() })}</option>
                          {(input.options ?? []).map((opt) => (
                            <option key={opt.value} value={opt.value}>
                              {opt.label}
                            </option>
                          ))}
                        </select>
                      );
                    case 'file_path':
                      return (
                        <div className="flex gap-2">
                          <input
                            type="text"
                            value={currentValue}
                            onChange={(e) => handleChange(e.target.value)}
                            placeholder={
                              input.placeholder ||
                              (isTauri() ? t('configModal.selectFile') : 'Enter absolute path')
                            }
                            className="input w-full"
                            data-testid={`config-input-${input.id}`}
                          />
                          {isTauri() && (
                            <button
                              type="button"
                              className="btn btn-secondary shrink-0 px-2"
                              onClick={async () => {
                                const selected = await pickPath({ multiple: false, directory: false });
                                if (typeof selected === 'string' && selected.length > 0) {
                                  handleChange(selected);
                                }
                              }}
                            >
                              <FolderOpen className="w-4 h-4" />
                            </button>
                          )}
                        </div>
                      );
                    case 'directory_path':
                      return (
                        <div className="flex gap-2">
                          <input
                            type="text"
                            value={currentValue}
                            onChange={(e) => handleChange(e.target.value)}
                            placeholder={
                              input.placeholder ||
                              (isTauri() ? t('configModal.selectDirectory') : 'Enter absolute path')
                            }
                            className="input w-full"
                            data-testid={`config-input-${input.id}`}
                          />
                          {isTauri() && (
                            <button
                              type="button"
                              className="btn btn-secondary shrink-0 px-2"
                              onClick={async () => {
                                const selected = await pickPath({ directory: true, multiple: false });
                                if (typeof selected === 'string' && selected.length > 0) {
                                  handleChange(selected);
                                }
                              }}
                            >
                              <FolderOpen className="w-4 h-4" />
                            </button>
                          )}
                        </div>
                      );
                    case 'text':
                    default:
                      return (
                        <input
                          type={input.secret ? 'password' : 'text'}
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          placeholder={input.placeholder || t('configModal.enterOption', { label: input.label.toLowerCase() })}
                          className="input w-full"
                          data-testid={`config-input-${input.id}`}
                        />
                      );
                  }
                };
                
                return (
                  <div key={input.id}>
                    <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                      {input.label}
                      {input.required && <span className="text-[rgb(var(--error))] ml-1">*</span>}
                    </label>
                    {input.description && (
                      <p className="text-xs text-[rgb(var(--muted))] mb-2">{input.description}</p>
                    )}
                    {renderInput()}
                    {obtainUrl && (
                      <a
                        href={obtainUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-xs text-[rgb(var(--primary))] hover:underline mt-1 inline-block"
                      >
                        {obtainInstructions || t('configModal.getKey')}
                      </a>
                    )}
                  </div>
                );
              })}

              {/* Additional Arguments (stdio only) */}
              {configModal.server.transport.type === 'stdio' && (
                <div>
                  <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                    {t('configModal.additionalArgs')}
                  </label>
                  <p className="text-xs text-[rgb(var(--muted))] mb-2">
                    {t('configModal.additionalArgsDesc')}
                  </p>
                  <textarea
                    value={configModal.argsAppend.join('\n')}
                    onChange={(e) => {
                      const lines = e.target.value.split('\n');
                      setConfigModal({
                        ...configModal,
                        argsAppend: lines.filter((l) => l.length > 0 || e.target.value.endsWith('\n')),
                      });
                    }}
                    onBlur={(e) => {
                      // Clean up empty lines on blur
                      setConfigModal({
                        ...configModal,
                        argsAppend: e.target.value.split('\n').filter((l) => l.trim().length > 0),
                      });
                    }}
                    placeholder={t('configModal.argsPlaceholder')}
                    rows={3}
                    className="input w-full font-mono text-sm resize-y"
                    data-testid="config-args-append"
                  />
                </div>
              )}

              {/* Environment Variable Overrides */}
              <div data-testid="config-env-section">
                <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                  {t('configModal.envVars')}
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mb-2">
                  {configModal.server.transport.type === 'stdio'
                    ? t('configModal.envVarsStdio')
                    : t('configModal.envVarsDefault')}
                </p>
                <div className="space-y-2">
                  {Object.entries(configModal.envOverrides).map(([key, value], idx) => (
                    <div key={idx} className="flex gap-2">
                      <input
                        type="text"
                        value={key}
                        onChange={(e) => {
                          const entries = Object.entries(configModal.envOverrides);
                          entries[idx] = [e.target.value, value];
                          setConfigModal({
                            ...configModal,
                            envOverrides: Object.fromEntries(entries),
                          });
                        }}
                        placeholder={t('configModal.keyPlaceholder')}
                        className="input flex-1 font-mono text-sm"
                      />
                      <input
                        type="text"
                        value={value}
                        onChange={(e) => {
                          setConfigModal({
                            ...configModal,
                            envOverrides: { ...configModal.envOverrides, [key]: e.target.value },
                          });
                        }}
                        placeholder={t('configModal.valuePlaceholder')}
                        className="input flex-1 font-mono text-sm"
                      />
                      <button
                        onClick={() => {
                          // eslint-disable-next-line @typescript-eslint/no-unused-vars
                          const { [key]: _, ...rest } = configModal.envOverrides;
                          setConfigModal({ ...configModal, envOverrides: rest });
                        }}
                        className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
                        title={t('configModal.remove')}
                      >
                        ✕
                      </button>
                    </div>
                  ))}
                  <button
                    onClick={() => {
                      setConfigModal({
                        ...configModal,
                        envOverrides: { ...configModal.envOverrides, '': '' },
                      });
                    }}
                    className="text-xs text-[rgb(var(--primary))] hover:underline"
                    data-testid="config-add-env"
                  >
                    {t('configModal.addVariable')}
                  </button>
                </div>
              </div>

              {/* Extra HTTP Headers (http only) */}
              {configModal.server.transport.type === 'http' && (
                <div data-testid="config-headers-section">
                  <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                    {t('configModal.httpHeaders')}
                  </label>
                  <p className="text-xs text-[rgb(var(--muted))] mb-2">
                    {t('configModal.httpHeadersDesc')}
                  </p>
                  <div className="space-y-2">
                    {Object.entries(configModal.extraHeaders).map(([key, value], idx) => (
                      <div key={idx} className="flex gap-2">
                        <input
                          type="text"
                          value={key}
                          onChange={(e) => {
                            const entries = Object.entries(configModal.extraHeaders);
                            entries[idx] = [e.target.value, value];
                            setConfigModal({
                              ...configModal,
                              extraHeaders: Object.fromEntries(entries),
                            });
                          }}
                          placeholder={t('configModal.headerNamePlaceholder')}
                          className="input flex-1 font-mono text-sm"
                        />
                        <input
                          type="text"
                          value={value}
                          onChange={(e) => {
                            setConfigModal({
                              ...configModal,
                              extraHeaders: { ...configModal.extraHeaders, [key]: e.target.value },
                            });
                          }}
                          placeholder={t('configModal.valuePlaceholder')}
                          className="input flex-1 font-mono text-sm"
                        />
                        <button
                          onClick={() => {
                            // eslint-disable-next-line @typescript-eslint/no-unused-vars
                            const { [key]: _, ...rest } = configModal.extraHeaders;
                            setConfigModal({ ...configModal, extraHeaders: rest });
                          }}
                          className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
                          title={t('configModal.remove')}
                        >
                          ✕
                        </button>
                      </div>
                    ))}
                    <button
                      onClick={() => {
                        setConfigModal({
                          ...configModal,
                          extraHeaders: { ...configModal.extraHeaders, '': '' },
                        });
                      }}
                      className="text-xs text-[rgb(var(--primary))] hover:underline"
                      data-testid="config-add-header"
                    >
                      {t('configModal.addHeader')}
                    </button>
                  </div>
                </div>
              )}

              {/* Default Tool Parameters */}
              {configModal.server.transport.type === 'stdio' &&
                isPackageManagedTransport(configModal.server.transport.command) && (
                  <div className="space-y-4 border-t border-[rgb(var(--border-subtle))] pt-4">
                    <div>
                      <label
                        htmlFor="config-update-policy"
                        className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
                      >
                        {t('configModal.updatePolicy')}
                      </label>
                      <p className="text-xs text-[rgb(var(--muted))] mb-2">
                        {
                          updatePolicyOptions.find(
                            (option) => option.value === configModal.updatePolicy
                          )?.description
                        }
                      </p>
                      <select
                        id="config-update-policy"
                        value={configModal.updatePolicy}
                        onChange={(e) =>
                          setConfigModal({
                            ...configModal,
                            updatePolicy: e.target.value as UpdatePolicy,
                          })
                        }
                        className="input w-full"
                        data-testid="config-update-policy"
                      >
                        {updatePolicyOptions.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </div>

                    {configModal.updatePolicy === 'pinned' && (
                      <div>
                        <label
                          htmlFor="config-pinned-version"
                          className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
                        >
                          {t('configModal.pinnedVersion')}
                        </label>
                        <p className="text-xs text-[rgb(var(--muted))] mb-2">
                          {t('configModal.pinnedVersionDesc')}
                        </p>
                        <input
                          id="config-pinned-version"
                          type="text"
                          value={configModal.pinnedVersion}
                          onChange={(e) =>
                            setConfigModal({ ...configModal, pinnedVersion: e.target.value })
                          }
                          placeholder="1.2.3"
                          className="input w-full font-mono text-sm"
                          data-testid="config-pinned-version"
                        />
                        {configModal.pinnedVersion.trim().length > 0 &&
                          !isValidSemver(configModal.pinnedVersion) && (
                            <p className="text-xs text-[rgb(var(--error))] mt-1">
                              {t('configModal.invalidSemver')}
                            </p>
                          )}
                      </div>
                    )}

                    {(configModal.server.latest_available_version ||
                      configModal.server.version_checked_at) && (
                      <div className="text-xs text-[rgb(var(--muted))]">
                        {(() => {
                          const currentVersion = resolveCurrentPackageVersion({
                            pinnedVersion:
                              configModal.pinnedVersion || configModal.server.pinned_version,
                            transportCommand: configModal.server.transport.command,
                            transportArgs: configModal.server.transport.args,
                            installedVersion: configModal.server.current_version,
                          });
                          if (!currentVersion) {
                            return null;
                          }
                          return (
                            <p>
                              {t('configModal.current')}{' '}
                              <span className="font-mono text-[rgb(var(--foreground))]">
                                {currentVersion}
                              </span>
                            </p>
                          );
                        })()}
                        {configModal.server.latest_available_version && (
                          <p>
                            {t('configModal.latest')}{' '}
                            <span className="font-mono text-[rgb(var(--foreground))]">
                              {configModal.server.latest_available_version}
                            </span>
                          </p>
                        )}
                      </div>
                    )}
                  </div>
                )}

              <div>
                <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                  {t('configModal.defaultParams')}
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mb-2">
                  {t('configModal.defaultParamsDesc')}{' '}
                  <code className="font-mono">{`{"cloudId": "abc123"}`}</code>
                </p>
                <textarea
                  value={configModal.defaultParamsJson}
                  onChange={(e) =>
                    setConfigModal({ ...configModal, defaultParamsJson: e.target.value })
                  }
                  placeholder="{}"
                  rows={3}
                  className="input w-full font-mono text-sm resize-y"
                  data-testid="config-default-params"
                  spellCheck={false}
                />
                <div className="flex items-center gap-2 mt-2">
                  <label className="text-xs text-[rgb(var(--muted))]">{t('configModal.onCollision')}</label>
                  <select
                    value={configModal.defaultParamsStrategy}
                    onChange={(e) =>
                      setConfigModal({
                        ...configModal,
                        defaultParamsStrategy: e.target.value as 'fill' | 'override',
                      })
                    }
                    className="text-xs border border-[rgb(var(--border))] rounded px-2 py-1 bg-[rgb(var(--surface))] text-[rgb(var(--foreground))]"
                    data-testid="config-default-params-strategy"
                  >
                    <option value="fill">{t('configModal.callerWins')}</option>
                    <option value="override">{t('configModal.defaultsWin')}</option>
                  </select>
                </div>
              </div>

              <div className="flex justify-end gap-2 pt-2">
                <button
                  onClick={handleCancelConfig}
                  className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                  data-testid="config-cancel-btn"
                >
                  {t('configModal.cancel')}
                </button>
                <button
                  onClick={handleSaveConfig}
                  disabled={
                    (configModal.server.transport.metadata?.inputs ?? [])
                      .some((i: InputDefinition) => i.required && !configModal.inputValues[i.id]) ||
                    (configModal.updatePolicy === 'pinned' &&
                      (!configModal.pinnedVersion.trim() ||
                        !isValidSemver(configModal.pinnedVersion)))
                  }
                  className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] disabled:opacity-50 transition-colors"
                  data-testid="config-save-btn"
                >
                  {configModal.enableOnSave && !configModal.server.enabled 
                    ? t('configModal.saveAndEnable')
                    : configModal.server.enabled 
                      ? t('configModal.saveAndReconnect')
                      : t('configModal.save')
                  }
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Bottom Toast Notification */}
      {toast && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 animate-in slide-in-from-bottom-2 duration-200">
          <div className={`flex items-center gap-3 px-4 py-3 rounded-lg shadow-lg border backdrop-blur-sm ${
            toast.type === 'success' 
              ? 'bg-[rgb(var(--success))]/90 border-[rgb(var(--success))] text-white'
              : toast.type === 'error'
              ? 'bg-[rgb(var(--error))]/90 border-[rgb(var(--error))] text-white'
              : 'bg-[rgb(var(--primary))]/90 border-[rgb(var(--primary))] text-white'
          }`}>
            {toast.type === 'success' && <span className="text-lg">✓</span>}
            {toast.type === 'error' && <span className="text-lg">✕</span>}
            {toast.type === 'info' && <span className="text-lg">ℹ</span>}
            <span className="text-sm font-medium">{toast.message}</span>
            <button 
              onClick={() => setToast(null)}
              className="ml-2 hover:opacity-70 transition-opacity"
            >
              ✕
            </button>
          </div>
        </div>
      )}
      
      {/* Log Viewer Modal */}
      {logViewerServer && (
        <ServerLogViewer
          serverId={logViewerServer.id}
          serverName={logViewerServer.name}
          onClose={() => setLogViewerServer(null)}
        />
      )}
      
      {/* Definition Viewer Modal */}
      {definitionServer && (() => {
        const server = installedServers.find(s => s.id === definitionServer.id);
        return server ? (
          <ServerDefinitionModal
            server={server}
            onClose={() => setDefinitionServer(null)}
          />
        ) : null;
      })()}

      {/* Config Editor Modal */}
      {editConfigSpace && (
        <ConfigEditorModal
          spaceId={editConfigSpace.id}
          spaceName={editConfigSpace.name}
          insertNewServer
          onClose={() => setEditConfigSpace(null)}
          onSaved={() => {
            loadData(); // Reload servers after config save
          }}
        />
      )}
    </div>
  );
}
