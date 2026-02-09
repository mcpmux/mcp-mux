/**
 * Server Install Modal
 *
 * Displays when a deep link install request is received from the discovery UI.
 *
 * ## Flow
 * 1. Deep link received with serverId only
 * 2. Look up server definition from registry
 * 3. Show modal with server info and space picker
 * 4. On confirm, call install_server command
 */

import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, Check, X, AlertCircle, Loader2, Info } from 'lucide-react';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { listSpaces, type Space } from '@/lib/api/spaces';
import {
  getServerDefinition,
  installServer,
  listInstalledServers,
} from '@/lib/api/registry';
import type { ServerDefinition } from '@/types/registry';
import { useViewSpace } from '@/stores';

/** Deep link payload from backend */
interface ServerInstallDeepLinkPayload {
  serverId: string;
}

/** Modal state machine */
type ModalState =
  | { type: 'hidden' }
  | { type: 'loading'; serverId: string }
  | { type: 'error'; serverId: string; message: string }
  | { type: 'ready'; server: ServerDefinition; alreadyInstalled: boolean }
  | { type: 'success'; serverName: string };

export function ServerInstallModal() {
  const [modalState, setModalState] = useState<ModalState>({ type: 'hidden' });
  const [selectedSpaceId, setSelectedSpaceId] = useState<string | null>(null);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [isInstalling, setIsInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  const viewSpace = useViewSpace();

  // Listen for deep link events
  useEffect(() => {
    const unlisten = listen<ServerInstallDeepLinkPayload>(
      'server-install-request',
      async (event) => {
        const { serverId } = event.payload;
        console.log('[Install] Deep link received for server:', serverId);

        setModalState({ type: 'loading', serverId });
        setInstallError(null);
        setIsInstalling(false);

        try {
          const [spacesResult, serverDef] = await Promise.all([
            listSpaces(),
            getServerDefinition(serverId),
          ]);

          setSpaces(spacesResult);

          // Default to current view space
          const defaultSpaceId = viewSpace?.id ?? spacesResult[0]?.id ?? null;
          setSelectedSpaceId(defaultSpaceId);

          if (!serverDef) {
            setModalState({
              type: 'error',
              serverId,
              message: `Server "${serverId}" was not found in the registry.`,
            });
            return;
          }

          // Check if already installed in the default space
          let alreadyInstalled = false;
          if (defaultSpaceId) {
            const installed = await listInstalledServers(defaultSpaceId);
            alreadyInstalled = installed.some((s) => s.server_id === serverId);
          }

          setModalState({ type: 'ready', server: serverDef, alreadyInstalled });
        } catch (err) {
          console.error('[Install] Failed to load server details:', err);
          setModalState({
            type: 'error',
            serverId,
            message: String(err),
          });
        }
      }
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [viewSpace?.id]);

  // Re-check install status when space selection changes
  useEffect(() => {
    if (modalState.type !== 'ready' || !selectedSpaceId) return;

    listInstalledServers(selectedSpaceId)
      .then((installed) => {
        const alreadyInstalled = installed.some(
          (s) => s.server_id === modalState.server.id
        );
        if (alreadyInstalled !== modalState.alreadyInstalled) {
          setModalState({ ...modalState, alreadyInstalled });
        }
      })
      .catch(console.error);
  }, [selectedSpaceId]);

  const handleInstall = async () => {
    if (modalState.type !== 'ready' || !selectedSpaceId) return;

    setIsInstalling(true);
    setInstallError(null);

    try {
      await installServer(modalState.server.id, selectedSpaceId);
      console.log('[Install] Server installed:', modalState.server.id);
      setModalState({ type: 'success', serverName: modalState.server.name });

      // Auto-dismiss after 2 seconds
      setTimeout(() => setModalState({ type: 'hidden' }), 2000);
    } catch (err) {
      console.error('[Install] Failed to install server:', err);
      setInstallError(String(err));
    } finally {
      setIsInstalling(false);
    }
  };

  const handleDismiss = () => {
    setModalState({ type: 'hidden' });
    setInstallError(null);
  };

  // Hidden
  if (modalState.type === 'hidden') return null;

  // Loading
  if (modalState.type === 'loading') {
    return (
      <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" data-testid="install-modal-loading">
        <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
          <CardContent className="py-8 flex flex-col items-center gap-4">
            <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            <p className="text-[rgb(var(--muted))]">Looking up server...</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Error
  if (modalState.type === 'error') {
    return (
      <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" data-testid="install-modal-error">
        <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-full bg-red-500/10">
                <AlertCircle className="h-6 w-6 text-red-500" />
              </div>
              <div>
                <CardTitle>Server Not Found</CardTitle>
                <CardDescription>
                  Could not find the requested server
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-[rgb(var(--muted))]" data-testid="install-modal-error-message">
              {modalState.message}
            </p>
            <Button onClick={handleDismiss} className="w-full" data-testid="install-modal-close-btn">
              Close
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Success
  if (modalState.type === 'success') {
    return (
      <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" data-testid="install-modal-success">
        <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
          <CardContent className="py-8 flex flex-col items-center gap-4">
            <div className="p-3 rounded-full bg-green-500/10">
              <Check className="h-8 w-8 text-green-500" />
            </div>
            <div className="text-center">
              <p className="font-medium text-lg">Installed!</p>
              <p className="text-sm text-[rgb(var(--muted))] mt-1" data-testid="install-modal-success-message">
                {modalState.serverName} has been added to your space.
              </p>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Ready - main install modal
  const { server, alreadyInstalled } = modalState;

  return (
    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" data-testid="install-modal">
      <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
        <CardHeader>
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-full bg-primary-500/10">
              <Download className="h-6 w-6 text-primary-500" />
            </div>
            <div>
              <CardTitle>Install Server</CardTitle>
              <CardDescription>
                Add this server to your space
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Server Info */}
          <div className="p-4 rounded-lg bg-surface-hover border border-[rgb(var(--border))]" data-testid="install-modal-server-info">
            <div className="flex items-center gap-3">
              {server.icon && (
                <span className="text-2xl">{server.icon}</span>
              )}
              <div className="flex-1 min-w-0">
                <div className="font-medium text-lg" data-testid="install-modal-server-name">{server.name}</div>
                {server.description && (
                  <div className="text-sm text-[rgb(var(--muted))] mt-0.5 line-clamp-2">
                    {server.description}
                  </div>
                )}
              </div>
            </div>
            {/* Transport badge */}
            <div className="mt-3 flex items-center gap-2">
              <span className="px-2 py-0.5 text-xs rounded-full bg-primary-500/10 text-primary-500 border border-primary-500/20">
                {server.transport.type === 'stdio' ? 'Local' : 'Remote'}
              </span>
              {server.auth && server.auth.type !== 'none' && (
                <span className="px-2 py-0.5 text-xs rounded-full bg-amber-500/10 text-amber-600 border border-amber-500/20">
                  {server.auth.type === 'oauth' ? 'OAuth' : 'API Key'}
                </span>
              )}
            </div>
          </div>

          {/* Already Installed Warning */}
          {alreadyInstalled && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-blue-500/10 text-blue-500 text-sm" data-testid="install-modal-already-installed">
              <Info className="h-4 w-4 flex-shrink-0" />
              <span>This server is already installed in the selected space.</span>
            </div>
          )}

          {/* Space Picker */}
          <div>
            <label className="text-sm font-medium mb-1 block">
              Install to space
            </label>
            {spaces.length > 0 ? (
              <select
                value={selectedSpaceId || ''}
                onChange={(e) => setSelectedSpaceId(e.target.value || null)}
                className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-primary-500/20"
                data-testid="install-modal-space-select"
              >
                {spaces.map((space) => (
                  <option key={space.id} value={space.id}>
                    {space.icon ? `${space.icon} ${space.name}` : space.name}
                  </option>
                ))}
              </select>
            ) : (
              <p className="text-sm text-[rgb(var(--muted))]">
                No spaces available. Create a space first.
              </p>
            )}
          </div>

          {/* Install Error */}
          {installError && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 text-red-500 text-sm" data-testid="install-modal-install-error">
              <AlertCircle className="h-4 w-4 flex-shrink-0" />
              <span>{installError}</span>
            </div>
          )}

          {/* Action Buttons */}
          <div className="flex gap-3 pt-2">
            <Button
              variant="secondary"
              className="flex-1"
              onClick={handleDismiss}
              disabled={isInstalling}
              data-testid="install-modal-cancel-btn"
            >
              <X className="h-4 w-4 mr-2" />
              Cancel
            </Button>
            <Button
              variant="primary"
              className="flex-1"
              onClick={handleInstall}
              disabled={isInstalling || alreadyInstalled || !selectedSpaceId}
              data-testid="install-modal-install-btn"
            >
              {isInstalling ? (
                <div className="h-4 w-4 mr-2 animate-spin rounded-full border-2 border-current border-t-transparent" />
              ) : (
                <Download className="h-4 w-4 mr-2" />
              )}
              Install
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
