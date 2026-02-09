/**
 * Servers page for managing installed MCP servers and their connections.
 * 
 * Uses event-driven ServerManager for:
 * - Real-time status updates via Tauri events
 * - Connect/Reconnect/Cancel button logic
 * - Auth progress display during OAuth
 */

import { useEffect, useState, useCallback } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Wrench,
  MessageSquare,
  FileText,
  Loader2,
  Clock,
  FileJson,
} from 'lucide-react';
import { ServerActionMenu } from './ServerActionMenu';
import type { ServerViewModel, ServerDefinition, InstalledServerState, InputDefinition } from '../../types/registry';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { listServerFeaturesByServer } from '@/lib/api/serverFeatures';
import type { ConnectionStatus } from '@/lib/api/serverManager';
import { useViewSpace } from '@/stores';
import { useServerManager } from '@/hooks/useServerManager';
import { useGatewayEvents, useDomainEvents } from '@/hooks/useDomainEvents';
import type { GatewayChangedPayload, ServerChangedPayload } from '@/hooks/useDomainEvents';
import type { FeaturesUpdatedEvent } from '@/lib/api/serverManager';
import { ServerLogViewer } from '@/components/ServerLogViewer';
import { ConfigEditorModal } from '@/components/ConfigEditorModal';
import { SourceBadge } from '@/components/SourceBadge';

// Helper to merge definitions with states (same as registryStore)
function mergeDefinitionsWithStates(
  definitions: ServerDefinition[],
  states: InstalledServerState[]
): ServerViewModel[] {
  const stateMap = new Map(states.map(s => [s.server_id, s]));
  
  return definitions.map(def => {
    const state = stateMap.get(def.id);
    
    // Check if any required inputs are missing
    const inputs = def.transport.metadata?.inputs ?? [];
    const inputValues = state?.input_values ?? {};
    const missing_required_inputs = inputs.some((input: InputDefinition) =>
      input.required && !inputValues[input.id]
    );
    
    // Calculate initial connection_status based on enabled state
    // Calculate initial connection_status based on enabled state
    // Actual runtime status comes from ServerManager events via useServerManager hook
    const connection_status = state?.enabled ? 'connecting' : 'disconnected';
    
    return {
      ...def,
      is_installed: !!state,
      enabled: state?.enabled ?? false,
      oauth_connected: state?.oauth_connected ?? false,
      input_values: inputValues,
      connection_status, // Initial status, will be overridden by runtime events
      missing_required_inputs,
      last_error: null, // Runtime-only, will be set by ServerManager events
      created_at: state?.created_at, // Include for sorting
      installation_source: state?.source, // Track how server was installed
      env_overrides: state?.env_overrides ?? {},
      args_append: state?.args_append ?? [],
      extra_headers: state?.extra_headers ?? {},
    } as ServerViewModel;
  });
}

// Helper to create ServerViewModel from installed state when registry is unavailable
// Uses cached_definition if available (proper offline support), otherwise falls back to minimal data
function createOfflineServerViewModel(state: InstalledServerState): ServerViewModel {
  // Try to use cached definition first (proper offline support)
  if (state.cached_definition) {
    try {
      const definition: ServerDefinition = JSON.parse(state.cached_definition);
      const inputValues = state.input_values;
      const requiredInputs = definition.transport.metadata?.inputs?.filter((i) => i.required) || [];
      const missing_required_inputs = requiredInputs.some(
        (input) => !inputValues[input.id]
      );
      const connection_status = state.enabled
        ? (missing_required_inputs ? 'error' : 'connecting')
        : 'disconnected';

      return {
        ...definition,
        is_installed: true,
        enabled: state.enabled,
        oauth_connected: state.oauth_connected,
        input_values: inputValues,
        connection_status,
        missing_required_inputs,
        last_error: null,
        created_at: state.created_at,
        installation_source: state.source,
        env_overrides: state.env_overrides ?? {},
        args_append: state.args_append ?? [],
        extra_headers: state.extra_headers ?? {},
      } as ServerViewModel;
    } catch (e) {
      console.warn('[ServersPage] Failed to parse cached_definition, using minimal fallback:', e);
    }
  }

  // Fallback: minimal view model when no cached definition available
  return {
    id: state.server_id,
    name: state.server_name || state.server_id.split('/').pop() || state.server_id,
    description: '(Server definition not cached)',
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
    connection_status: state.enabled ? 'connecting' : 'disconnected',
    missing_required_inputs: false,
    last_error: null,
    created_at: state.created_at,
    installation_source: state.source,
    env_overrides: state.env_overrides ?? {},
    args_append: state.args_append ?? [],
    extra_headers: state.extra_headers ?? {},
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
}

export function ServersPage() {
  const [installedServers, setInstalledServers] = useState<ServerViewModel[]>([]);
  const [gatewayRunning, setGatewayRunning] = useState(false);
  const [gatewayUrl, setGatewayUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  // Bottom toast notifications
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' | 'info' } | null>(null);
  const [configModal, setConfigModal] = useState<ConfigModalState>({
    open: false,
    server: null,
    inputValues: {},
    envOverrides: {},
    argsAppend: [],
    extraHeaders: {},
  });

  // Features state
  const [serverFeatures, setServerFeatures] = useState<Record<string, ServerFeature[]>>({});
  const [expandedServers, setExpandedServers] = useState<Set<string>>(new Set());
  const [loadingFeatures, setLoadingFeatures] = useState<Set<string>>(new Set());
  
  // Log viewer state
  const [logViewerServer, setLogViewerServer] = useState<{ id: string; name: string } | null>(null);
  
  // Config editor state
  const [editConfigSpace, setEditConfigSpace] = useState<{ id: string; name: string } | null>(null);
  
  const viewSpace = useViewSpace();
  
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
  const showToast = (message: string, type: 'success' | 'error' | 'info' = 'info') => {
    setToast({ message, type });
    setTimeout(() => setToast(null), 5000);
  };

  // Load data on mount only
  useEffect(() => {
    loadData();
  }, []);

  useEffect(() => {
    setServerFeatures({});
    setExpandedServers(new Set());
    setLoadingFeatures(new Set());
    loadData();
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
        loadData();
      }
    });
  }, [viewSpace?.id]);

  // Note: Server status changes are handled by useServerManager hook
  // which updates serverStatuses state via events. No need to re-fetch
  // server definitions on status changes - they don't change.

  const loadData = async () => {
    try {
      setIsLoading(true);
      
      // Use allSettled so we can show installed servers even if registry is offline
      const [installedResult, gatewayResult, definitionsResult] = await Promise.allSettled([
        import('@/lib/api/registry').then((m) => m.listInstalledServers(viewSpace?.id)),
        import('@/lib/api/gateway').then((m) => m.getGatewayStatus(viewSpace?.id)),
        import('@/lib/api/registry').then((m) => m.discoverServers()),
      ]);
      
      // Extract values, using fallbacks for failures
      const installed = installedResult.status === 'fulfilled' ? installedResult.value : [];
      const gateway = gatewayResult.status === 'fulfilled' 
        ? gatewayResult.value 
        : { running: false, url: null };
      const definitions = definitionsResult.status === 'fulfilled' 
        ? definitionsResult.value 
        : [];

      
      // Log if registry is offline but we have installed servers
      if (definitionsResult.status === 'rejected' && installed.length > 0) {
        console.warn('[ServersPage] Registry offline, showing installed servers with cached/minimal info');
        showToast('Registry offline - showing cached server info', 'info');
      }
      
      // Merge definitions with installed states
      // If definitions are missing, create minimal ServerViewModels from installed states
      let mergedServers: ServerViewModel[];
      
      if (definitions.length > 0) {
        // Normal case: merge definitions with states
        const allMerged = mergeDefinitionsWithStates(definitions, installed);
        mergedServers = allMerged.filter(s => s.is_installed);

        // Handle installed servers not present in registry definitions
        // (e.g., registry changed, using different registry, or servers installed from user config)
        const matchedServerIds = new Set(mergedServers.map(s => s.id));
        const unmatchedInstalled = installed.filter(s => !matchedServerIds.has(s.server_id));
        if (unmatchedInstalled.length > 0) {
          const offlineViewModels = unmatchedInstalled.map(state => createOfflineServerViewModel(state));
          mergedServers = [...mergedServers, ...offlineViewModels];
        }
      } else {
        // Offline case: create minimal view models from installed states only
        mergedServers = installed.map(state => createOfflineServerViewModel(state));
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
    } catch (e) {
      console.error('Failed to load data:', e);
    } finally {
      setIsLoading(false);
    }
  };

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
   */
  const getServerAction = (server: ServerViewModel): 'enable' | 'configure' | 'connecting' | 'authenticating' | 'auth_required' | 'running' | 'error' | 'connected_auto' => {
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
          // Enabled but disconnected - try to connect
          return server.auth?.type === 'oauth' ? 'auth_required' : 'connected_auto';
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
    if (server.auth?.type === 'oauth') {
      return 'auth_required';
    }
    
    // Non-OAuth server that's enabled but not yet connected
    return 'connected_auto';
  };

  // Get display status for UI
  const getDisplayStatus = (server: ServerViewModel): string => {
    const action = getServerAction(server);
    const remainingSeconds = getAuthRemainingSeconds(server.id);
    
    switch (action) {
      case 'enable': return 'Disabled';
      case 'configure': return 'Needs Configuration';
      case 'connecting': return 'Connecting...';
      case 'authenticating': 
        if (remainingSeconds !== undefined) {
          const minutes = Math.floor(remainingSeconds / 60);
          const seconds = remainingSeconds % 60;
          return `Authenticating... (${minutes}m ${seconds}s)`;
        }
        return 'Authenticating...';
      case 'auth_required': 
        return hasConnectedBefore(server.id) ? 'Reconnect Required' : 'Connect Required';
      case 'running': return 'Connected';
      case 'connected_auto': return 'Connected';
      case 'error': return 'Error';
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
      setConfigModal({
        open: true,
        server,
        inputValues: initialValues,
        enableOnSave: true, // This is from Enable flow
        envOverrides: { ...(server.env_overrides ?? {}) },
        argsAppend: [...(server.args_append ?? [])],
        extraHeaders: { ...(server.extra_headers ?? {}) },
      });
      return;
    }

    setActionLoading(`enable-${server.id}`);
    try {
      // Use new ServerManager v2 - handles connection + OAuth in backend
      await enableServerV2(server.id);
      await loadData();
      
      // Expand server to show features after connection
      setTimeout(() => {
        setExpandedServers(prev => new Set(prev).add(server.id));
        loadFeaturesForServer(server.id);
      }, 1000);
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
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

  // Handle Configure button click (from overflow menu or pending_config state)
  const handleConfigureClick = (server: ServerViewModel) => {
    const serverInputs = server.transport.metadata?.inputs ?? [];
    const initialValues: Record<string, string> = {};
    serverInputs.forEach((input: InputDefinition) => {
      initialValues[input.id] = server.input_values[input.id] || '';
    });
    setConfigModal({
      open: true,
      server,
      inputValues: initialValues,
      enableOnSave: false, // Just configure, don't enable
      envOverrides: { ...(server.env_overrides ?? {}) },
      argsAppend: [...(server.args_append ?? [])],
      extraHeaders: { ...(server.extra_headers ?? {}) },
    });
  };

  const handleSaveConfig = async () => {
    if (!configModal.server) return;
    
    const server = configModal.server;
    const serverId = server.id;
    const shouldEnable = configModal.enableOnSave ?? false;
    
    setActionLoading(`config-${serverId}`);
    try {
      const { saveServerInputs } = await import('@/lib/api/registry');

      // Save input values with env overrides, args, and headers.
      // Always send the values (even if empty) so that clearing them works.
      // Backend treats None as "keep existing", so we must send Some({}/[])
      // to actually clear fields the user removed.
      await saveServerInputs(
        serverId,
        configModal.inputValues,
        viewSpace?.id ?? '',
        configModal.envOverrides,
        configModal.argsAppend,
        configModal.extraHeaders,
      );

      setConfigModal({ open: false, server: null, inputValues: {}, envOverrides: {}, argsAppend: [], extraHeaders: {} });
      
      // Only enable if requested (from Enable flow)
      if (shouldEnable && !server.enabled) {
        // Use new ServerManager v2 to enable and connect
        await enableServerV2(serverId);
        
        // Reload data AFTER enable to get updated enabled state
        await loadData();
        
        setTimeout(() => {
          setExpandedServers(prev => new Set(prev).add(serverId));
          loadFeaturesForServer(serverId);
        }, 1000);
      } else if (server.enabled) {
        // If already enabled, trigger reconnect with new config
        await retryConnectionV2(serverId);
        await loadData();
      } else {
        // Just save, don't enable
        await loadData();
      }
      
      showToast('Configuration saved', 'success');
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
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
    setConfigModal({ open: false, server: null, inputValues: {}, envOverrides: {}, argsAppend: [], extraHeaders: {} });
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

  const handleUninstall = async (server: ServerViewModel) => {
    // Import source-aware helpers
    const { getUninstallConfirmMessage, getUninstallLabel } = await import('@/components/SourceBadge');
    const confirmMsg = getUninstallConfirmMessage(server.name, server.installation_source);
    const actionLabel = getUninstallLabel(server.installation_source);
    
    if (!confirm(confirmMsg)) {
      return;
    }
    
    setActionLoading(`uninstall-${server.id}`);
    try {
      const { uninstallServer } = await import('@/lib/api/registry');
      const { disconnectServer } = await import('@/lib/api/gateway');
      
      if (gatewayRunning && server.enabled && viewSpace) {
        try {
          await disconnectServer(server.id, viewSpace.id);
        } catch (e) {
          console.warn(`[ServersPage] Failed to disconnect server from gateway:`, e);
        }
      }
      
      // ServerAppService handles source-aware cleanup automatically:
      // - UserConfig: removes from JSON file + DB
      // - Registry/ManualEntry: just removes from DB
      await uninstallServer(server.id, viewSpace?.id ?? '');
      await loadData();
      showToast(`${server.name} ${actionLabel.toLowerCase()}ed`, 'success');
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setActionLoading(null);
    }
  };

  const handleStartGateway = async () => {
    try {
      const { startGateway, connectAllEnabledServers } = await import('@/lib/api/gateway');
      const url = await startGateway();
      setGatewayRunning(true);
      setGatewayUrl(url);
      
      // Auto-connect all enabled servers
      try {
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
        showToast(`${server.name} disconnected and logged out`, 'info');
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
    <div className="space-y-6" data-testid="servers-page">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold" data-testid="servers-title">My Servers</h1>
          <p className="text-sm text-[rgb(var(--muted))]">
            Manage your installed MCP servers
          </p>
        </div>
        {viewSpace && (
          <button
            onClick={() => setEditConfigSpace({ id: viewSpace.id, name: viewSpace.name })}
            className="flex items-center gap-2 px-4 py-2 text-sm border border-[rgb(var(--border))] rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
          >
            <FileJson className="h-4 w-4" />
            Add Custom Server
          </button>
        )}
      </div>

      {/* Gateway Status */}
      <div
        className={`p-4 rounded-xl border ${
          gatewayRunning
            ? 'bg-[rgb(var(--success))]/10 border-[rgb(var(--success))]/30'
            : 'bg-[rgb(var(--warning))]/10 border-[rgb(var(--warning))]/30'
        }`}
      >
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <span className={`h-3 w-3 rounded-full ${gatewayRunning ? 'bg-[rgb(var(--success))] animate-pulse' : 'bg-[rgb(var(--warning))]'}`} />
            <span className="font-medium">
              {gatewayRunning ? 'Gateway Running' : 'Gateway Stopped'}
            </span>
            {gatewayRunning && (
              <code className="text-xs bg-[rgb(var(--surface-elevated))] px-2 py-1 rounded text-[rgb(var(--primary))]">
                {gatewayUrl}
              </code>
            )}
          </div>
          {!gatewayRunning && (
            <button
              onClick={handleStartGateway}
              className="px-4 py-2 text-sm bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] rounded-lg hover:bg-[rgb(var(--primary-hover))] transition-colors"
            >
              Start Gateway
            </button>
          )}
        </div>
      </div>

      {/* Server List */}
      {installedServers.length === 0 ? (
        <div className="text-center py-12 text-[rgb(var(--muted))]">
          <div className="text-5xl mb-4">📦</div>
          <p className="text-lg mb-2">No servers installed</p>
          <p className="text-sm">Visit Discover to install MCP servers</p>
        </div>
      ) : (
        <div className="space-y-3">
          {installedServers.map((server) => {
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
                className={`bg-[rgb(var(--card))] border border-[rgb(var(--border-subtle))] rounded-xl shadow-sm transition-all ${
                  !server.enabled && 'opacity-60'
                }`}
                data-testid={`installed-server-${server.id}`}
              >
                {/* Server Header */}
                <div className="p-4">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-4">
                      {/* Expand/Collapse button for connected servers */}
                      {isConnected && (
                        <button
                          onClick={() => toggleExpanded(server.id)}
                          className="p-1 rounded hover:bg-[rgb(var(--surface-hover))] transition-colors"
                          data-testid={`expand-server-${server.id}`}
                        >
                          {isExpanded ? (
                            <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))]" />
                          ) : (
                            <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))]" />
                          )}
                        </button>
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
                                  {counts.tools} tools
                                </span>
                              )}
                              {counts.prompts > 0 && (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs bg-blue-500/15 text-blue-600 dark:text-blue-400">
                                  <MessageSquare className="h-3 w-3" />
                                  {counts.prompts} prompts
                                </span>
                              )}
                              {counts.resources > 0 && (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs bg-green-500/15 text-green-600 dark:text-green-400">
                                  <FileText className="h-3 w-3" />
                                  {counts.resources} resources
                                </span>
                              )}
                            </>
                          )}
                          
                          {/* Auth Type Badge */}
                          {server.auth && server.auth.type !== 'none' && (
                            <span className="text-xs text-[rgb(var(--muted))] px-2 py-0.5 bg-[rgb(var(--surface-hover))] rounded-md">
                              {server.auth.type === 'oauth' ? '🔐 OAuth' : 
                               server.auth.type === 'api_key' ? '🔑 API Key' :
                               server.auth.type === 'optional_api_key' ? '🔑 API Key (Optional)' :
                               'Auth Required'}
                            </span>
                          )}
                          
                          <span className="text-xs text-[rgb(var(--muted))]">{server.transport.type}</span>
                          
                          {/* Installation Source Badge */}
                          <SourceBadge source={server.installation_source} />
                        </div>
                        
                        {/* Show runtime message inline (from ServerManager events) */}
                        {isAuthenticating && (
                          <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400">
                            <Clock className="h-3 w-3" />
                            <span>
                              {runtimeMessage || 'Waiting for browser authorization...'}
                            </span>
                          </div>
                        )}
                        
                        {/* Show error indicator if in error state */}
                        {serverAction === 'error' && (
                          <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg text-xs bg-[rgb(var(--error))]/10 text-[rgb(var(--error))]">
                            <span className="font-medium">Connection error</span>
                            <span className="text-[rgb(var(--muted))]">·</span>
                            <span className="text-[rgb(var(--muted))]">View logs for details</span>
                          </div>
                        )}
                      </div>
                    </div>

                    {/* Actions - horizontal row with primary and secondary actions */}
                    <div className="flex items-center gap-2">
                      {/* Primary action button */}
                      {serverAction === 'enable' && (
                        <button
                          onClick={() => handleEnableClick(server)}
                          disabled={enableLoading}
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
                          data-testid={`enable-server-${server.id}`}
                        >
                          {enableLoading ? 'Enabling...' : 'Enable'}
                        </button>
                      )}

                      {serverAction === 'configure' && (
                        <button
                          onClick={() => handleConfigureClick(server)}
                          disabled={configLoading}
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--warning))] text-white hover:bg-[rgb(var(--warning))]/90 transition-colors disabled:opacity-50"
                        >
                          {configLoading ? 'Saving...' : 'Configure'}
                        </button>
                      )}

                      {/* Connecting state - show spinner */}
                      {serverAction === 'connecting' && (
                        <button
                          disabled
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--surface-elevated))] text-[rgb(var(--muted))] cursor-not-allowed flex items-center gap-2"
                        >
                          <Loader2 className="h-4 w-4 animate-spin" />
                          Connecting...
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
                            Authenticating...
                          </button>
                          <button
                            onClick={() => handleCancelOAuth(server.id)}
                            className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                          >
                            Cancel
                          </button>
                        </>
                      )}
                      
                      {/* Auth Required state - show Connect/Reconnect button */}
                      {serverAction === 'auth_required' && gatewayRunning && (
                        <button
                          onClick={() => handleConnect(server)}
                          disabled={connectLoading}
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--success))] text-white hover:bg-[rgb(var(--success))]/90 transition-colors disabled:opacity-50"
                        >
                          {connectLoading ? 'Connecting...' : hasConnectedBefore(server.id) ? 'Reconnect' : 'Connect'}
                        </button>
                      )}

                      {/* Running state - show Disconnect button */}
                      {serverAction === 'running' && (
                        <button
                          onClick={() => handleDisconnect(server, false)}
                          disabled={actionLoading === `disconnect-${server.id}`}
                          className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-50"
                          title="Disconnect (keep credentials)"
                        >
                          {actionLoading === `disconnect-${server.id}` ? '...' : 'Disconnect'}
                        </button>
                      )}

                      {serverAction === 'error' && gatewayRunning && (
                        <button
                          onClick={() => handleRetry(server)}
                          disabled={retryLoading}
                          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--error))] text-white hover:bg-[rgb(var(--error))]/90 transition-colors disabled:opacity-50"
                        >
                          {retryLoading ? 'Retrying...' : hasConnectedBefore(server.id) ? 'Reconnect' : 'Retry'}
                        </button>
                      )}

                      {/* Disable button - shown when enabled and connected/running */}
                      {server.enabled && (serverAction === 'running' || serverAction === 'connected_auto') && (
                        <button
                          onClick={() => handleDisableClick(server)}
                          disabled={disableLoading}
                          className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-50"
                          data-testid={`disable-server-${server.id}`}
                        >
                          {disableLoading ? '...' : 'Disable'}
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
                        onConfigure={() => handleConfigureClick(server)}
                        onRefresh={() => handleRefresh(server)}
                        onReconnect={() => handleReconnect(server)}
                        onViewLogs={() => setLogViewerServer({ id: server.id, name: server.name })}
                        onUninstall={() => handleUninstall(server)}
                        disabled={!!actionLoading}
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
                        <span className="ml-2 text-sm text-[rgb(var(--muted))]">Loading features...</span>
                      </div>
                    ) : features.length === 0 ? (
                      <div className="text-center py-8 text-[rgb(var(--muted))]">
                        <p className="text-sm">No features discovered yet</p>
                        <p className="text-xs mt-1">Features will appear after the server initializes</p>
                        <button
                          onClick={() => loadFeaturesForServer(server.id)}
                          className="mt-3 px-3 py-1 text-xs rounded bg-[rgb(var(--surface-hover))] hover:bg-[rgb(var(--surface-active))] transition-colors"
                        >
                          Refresh
                        </button>
                      </div>
                    ) : (
                      <div className="p-4 space-y-4">
                        {/* Tools */}
                        {features.filter(f => f.feature_type === 'tool').length > 0 && (
                          <div>
                            <h4 className="text-sm font-medium flex items-center gap-2 mb-2">
                              <Wrench className="h-4 w-4 text-purple-500" />
                              Tools ({features.filter(f => f.feature_type === 'tool').length})
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
                              Prompts ({features.filter(f => f.feature_type === 'prompt').length})
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
                              Resources ({features.filter(f => f.feature_type === 'resource').length})
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

      {/* Configuration Modal */}
      {configModal.open && configModal.server && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4" data-testid="config-modal-overlay">
          <div className="dropdown-menu w-full max-w-md p-6 animate-in fade-in scale-in duration-150 max-h-[80vh] overflow-y-auto" data-testid="config-modal">
            <h3 className="text-lg font-semibold text-[rgb(var(--foreground))] mb-2" data-testid="config-modal-title">
              Configure {configModal.server.name}
            </h3>
            <p className="text-sm text-[rgb(var(--muted))] mb-4">
              {(configModal.server.auth && 'instructions' in configModal.server.auth ? configModal.server.auth.instructions : null) || 'Enter the required configuration to enable this server.'}
            </p>
            
            <div className="space-y-4">
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
                            {input.placeholder || 'Enable'}
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
                    case 'password':
                      return (
                        <input
                          type="password"
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          placeholder={input.placeholder || `Enter ${input.label.toLowerCase()}...`}
                          className="input w-full"
                          data-testid={`config-input-${input.id}`}
                        />
                      );
                    case 'text':
                    default:
                      return (
                        <input
                          type={input.secret ? 'password' : 'text'}
                          value={currentValue}
                          onChange={(e) => handleChange(e.target.value)}
                          placeholder={input.placeholder || `Enter ${input.label.toLowerCase()}...`}
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
                        {obtainInstructions || 'Get your key here →'}
                      </a>
                    )}
                  </div>
                );
              })}

              {/* Additional Arguments (stdio only) */}
              {configModal.server.transport.type === 'stdio' && (
                <div>
                  <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                    Additional Arguments
                  </label>
                  <p className="text-xs text-[rgb(var(--muted))] mb-2">
                    Extra command-line arguments (one per line)
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
                    placeholder="--flag&#10;value"
                    rows={3}
                    className="input w-full font-mono text-sm resize-y"
                    data-testid="config-args-append"
                  />
                </div>
              )}

              {/* Environment Variable Overrides */}
              <div>
                <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                  Environment Variables
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mb-2">
                  {configModal.server.transport.type === 'stdio'
                    ? 'Additional environment variables for the server process'
                    : 'Additional environment variables'}
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
                        placeholder="KEY"
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
                        placeholder="value"
                        className="input flex-1 font-mono text-sm"
                      />
                      <button
                        onClick={() => {
                          // eslint-disable-next-line @typescript-eslint/no-unused-vars
                          const { [key]: _, ...rest } = configModal.envOverrides;
                          setConfigModal({ ...configModal, envOverrides: rest });
                        }}
                        className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
                        title="Remove"
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
                    + Add variable
                  </button>
                </div>
              </div>

              {/* Extra HTTP Headers (http only) */}
              {configModal.server.transport.type === 'http' && (
                <div>
                  <label className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                    HTTP Headers
                  </label>
                  <p className="text-xs text-[rgb(var(--muted))] mb-2">
                    Custom HTTP headers sent with each request
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
                          placeholder="Header-Name"
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
                          placeholder="value"
                          className="input flex-1 font-mono text-sm"
                        />
                        <button
                          onClick={() => {
                            // eslint-disable-next-line @typescript-eslint/no-unused-vars
                            const { [key]: _, ...rest } = configModal.extraHeaders;
                            setConfigModal({ ...configModal, extraHeaders: rest });
                          }}
                          className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
                          title="Remove"
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
                      + Add header
                    </button>
                  </div>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <button
                  onClick={handleCancelConfig}
                  className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                  data-testid="config-cancel-btn"
                >
                  Cancel
                </button>
                <button
                  onClick={handleSaveConfig}
                  disabled={
                    (configModal.server.transport.metadata?.inputs ?? [])
                      .some((i: InputDefinition) => i.required && !configModal.inputValues[i.id])
                  }
                  className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] disabled:opacity-50 transition-colors"
                  data-testid="config-save-btn"
                >
                  {configModal.enableOnSave && !configModal.server.enabled 
                    ? 'Save & Enable' 
                    : configModal.server.enabled 
                      ? 'Save & Reconnect'
                      : 'Save'
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
      
      {/* Config Editor Modal */}
      {editConfigSpace && (
        <ConfigEditorModal
          spaceId={editConfigSpace.id}
          spaceName={editConfigSpace.name}
          onClose={() => setEditConfigSpace(null)}
          onSaved={() => {
            loadData(); // Reload servers after config save
          }}
        />
      )}
    </div>
  );
}
