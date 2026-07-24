import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import {
  createMachine,
  getHostname,
  getLocalMachineId,
  getViewerMachineId,
  listMachines,
  setLocalMachineId,
  setViewerMachineId,
  updateMachine,
  type Machine,
} from '@/lib/api/machines';
import {
  getMissingMachineProfileField,
  isMachineProfileComplete,
  toMachineProfilePayload,
} from '@/lib/machine-profile.helpers';
import { isMachineUuid } from '@/lib/machine-id.helpers';
import {
  clearViewerDeviceName,
  clearViewerMachineIdCache,
  getOrCreateViewerDeviceId,
  getViewerDeviceHints,
  getViewerDeviceName,
  getViewerMachineIdCache,
  isViewingLocally,
  setViewerMachineIdCache,
} from '@/lib/viewer-device.helpers';

export const VIEWER_IDENTITY_CHANGED = 'mcpmux-viewer-identity-changed';

/** Whether the identity modal is blocking setup, open for edits, or hidden. */
export type ViewerIdentityPromptMode = 'blocked' | 'edit' | 'closed';

interface ViewerMachineProfile {
  id: string;
  name: string;
  icon: string;
  hostname: string;
}

interface ViewerIdentityContextValue {
  name: string | null;
  icon: string;
  machineId: string | null;
  hints: string | null;
  isLoading: boolean;
  isSaving: boolean;
  promptMode: ViewerIdentityPromptMode;
  showPrompt: boolean;
  nameDraft: string;
  iconDraft: string;
  hostnameDraft: string;
  setNameDraft: (value: string) => void;
  setIconDraft: (value: string) => void;
  setHostnameDraft: (value: string) => void;
  error: string | null;
  setError: (value: string | null) => void;
  saveProfile: () => Promise<boolean>;
  canSaveProfile: boolean;
  openPrompt: () => void;
  closePrompt: () => void;
  prefillHostnameHint: () => Promise<void>;
  linkMachineIdDraft: string;
  setLinkMachineIdDraft: (value: string) => void;
  isLinking: boolean;
  linkError: string | null;
  linkMachineById: (id?: string) => Promise<boolean>;
}

/**
 * Normalize a machine catalog id for create-vs-update branching.
 */
function normalizeMachineId(id: string | null | undefined): string | null {
  const trimmed = id?.trim();
  return trimmed || null;
}

const ViewerIdentityContext = createContext<ViewerIdentityContextValue | null>(null);

/**
 * Provide shared viewer identity state to modal and status bar.
 */
export function ViewerIdentityProvider({ children }: { children: ReactNode }) {
  const value = useViewerIdentityState();
  return <ViewerIdentityContext.Provider value={value}>{children}</ViewerIdentityContext.Provider>;
}

/**
 * Read viewer identity state from the nearest provider.
 */
// eslint-disable-next-line react-refresh/only-export-components -- hook must live alongside its provider; splitting the file would break the shared private context reference
export function useViewerIdentity(): ViewerIdentityContextValue {
  const value = useContext(ViewerIdentityContext);
  if (!value) {
    throw new Error('useViewerIdentity must be used within ViewerIdentityProvider');
  }
  return value;
}

/**
 * Map a machine catalog row to viewer profile fields.
 */
function toViewerProfile(machine: Machine): ViewerMachineProfile {
  return {
    id: machine.id,
    name: machine.name,
    icon: machine.icon ?? '',
    hostname: machine.hostname ?? '',
  };
}

/**
 * Resolve the machine row linked to a remote viewer profile.
 */
async function resolveRemoteViewerMachine(viewerId: string): Promise<ViewerMachineProfile | null> {
  let machineId = getViewerMachineIdCache();
  if (!machineId) {
    machineId = await getViewerMachineId(viewerId);
  }

  if (!machineId) {
    return null;
  }

  const machines = await listMachines();
  const machine = machines.find((entry) => entry.id === machineId);
  if (!machine) {
    clearViewerMachineIdCache();
    await setViewerMachineId(viewerId, null);
    return null;
  }

  setViewerMachineIdCache(machine.id);
  return toViewerProfile(machine);
}

/**
 * Resolve this install's machine row when UI and gateway share a host.
 */
async function resolveLocalInstallMachine(): Promise<ViewerMachineProfile | null> {
  const localId = await getLocalMachineId();
  if (!localId) {
    return null;
  }

  const machines = await listMachines();
  const machine = machines.find((entry) => entry.id === localId);
  if (!machine) {
    return null;
  }

  setViewerMachineIdCache(machine.id);
  return toViewerProfile(machine);
}

/**
 * Drop a stale remote viewer mapping when local install identity wins.
 */
async function clearStaleRemoteViewerMapping(
  viewerId: string,
  localMachineId: string,
): Promise<void> {
  const viewerMapped = await getViewerMachineId(viewerId);
  if (viewerMapped && viewerMapped !== localMachineId) {
    await setViewerMachineId(viewerId, null);
  }
  if (getViewerMachineIdCache() && getViewerMachineIdCache() !== localMachineId) {
    clearViewerMachineIdCache();
    setViewerMachineIdCache(localMachineId);
  }
}

/**
 * Manage viewer device identity backed by the machine catalog.
 */
function useViewerIdentityState(): ViewerIdentityContextValue {
  const [viewerId] = useState(() => getOrCreateViewerDeviceId());
  const [name, setName] = useState<string | null>(null);
  const [machineId, setMachineId] = useState<string | null>(null);
  const [icon, setIcon] = useState('');
  const [hostname, setHostname] = useState('');
  const [hints, setHints] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [promptMode, setPromptMode] = useState<ViewerIdentityPromptMode>('closed');
  const [nameDraft, setNameDraft] = useState('');
  const [iconDraft, setIconDraft] = useState('');
  const [hostnameDraft, setHostnameDraft] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [linkMachineIdDraft, setLinkMachineIdDraft] = useState('');
  const [isLinking, setIsLinking] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);

  const applyMachine = useCallback(
    (machine: ViewerMachineProfile | null, options?: { preservePrompt?: boolean }) => {
      const normalizedId = normalizeMachineId(machine?.id);
      setMachineId(normalizedId);
      setName(machine?.name ?? null);
      setIcon(machine?.icon ?? '');
      setHostname(machine?.hostname ?? '');

      if (options?.preservePrompt) {
        return;
      }

      const isComplete =
        machine != null &&
        isMachineProfileComplete({
          name: machine.name,
          icon: machine.icon,
          hostname: machine.hostname,
        });

      if (!isComplete) {
        setPromptMode('blocked');
      } else {
        setPromptMode((current) => (current === 'edit' ? 'edit' : 'closed'));
      }

      setNameDraft(machine?.name ?? '');
      setIconDraft(machine?.icon ?? '');
      setHostnameDraft(machine?.hostname ?? '');
    },
    [],
  );

  const refreshIdentity = useCallback(async (options?: { preservePrompt?: boolean }) => {
    setIsLoading(true);
    try {
      const viewingLocally = isViewingLocally();
      let machine: ViewerMachineProfile | null = null;

      if (viewingLocally) {
        machine = await resolveLocalInstallMachine();

        if (machine) {
          await clearStaleRemoteViewerMapping(viewerId, machine.id);
        } else {
          machine = await resolveRemoteViewerMachine(viewerId);
          if (machine) {
            await setLocalMachineId(machine.id);
          }
        }
      } else {
        machine = await resolveRemoteViewerMachine(viewerId);
      }

      if (!machine) {
        const legacyName = getViewerDeviceName();
        if (legacyName) {
          applyMachine({ id: '', name: legacyName, icon: '', hostname: '' }, options);
          return;
        }
      }

      applyMachine(machine, options);
    } catch {
      const legacyName = getViewerDeviceName();
      applyMachine(
        legacyName ? { id: '', name: legacyName, icon: '', hostname: '' } : null,
        options,
      );
    } finally {
      setIsLoading(false);
    }
  }, [applyMachine, viewerId]);

  useEffect(() => {
    void getViewerDeviceHints().then(setHints);
    void refreshIdentity();

    const onChanged = () => {
      void refreshIdentity({ preservePrompt: promptMode === 'edit' });
    };
    window.addEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
    return () => window.removeEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
  }, [promptMode, refreshIdentity]);

  const saveProfile = useCallback(async () => {
    const missingField = getMissingMachineProfileField({
      name: nameDraft,
      icon: iconDraft,
      hostname: hostnameDraft,
    });
    if (missingField) {
      setError(missingField);
      return false;
    }

    const payload = toMachineProfilePayload({
      name: nameDraft,
      icon: iconDraft,
      hostname: hostnameDraft,
    });
    const viewingLocally = isViewingLocally();

    setIsSaving(true);
    setError(null);
    try {
      let targetMachineId = normalizeMachineId(machineId);
      let linkedLocalMachineId: string | null = null;
      if (!targetMachineId && viewingLocally) {
        linkedLocalMachineId = normalizeMachineId(await getLocalMachineId());
        targetMachineId = linkedLocalMachineId;
      }

      const machine = targetMachineId
        ? await updateMachine(targetMachineId, payload)
        : await createMachine(payload);

      if (viewingLocally) {
        if (!normalizeMachineId(machineId) && !linkedLocalMachineId) {
          await setLocalMachineId(machine.id);
        }
        await setViewerMachineId(viewerId, null);
      } else if (!normalizeMachineId(machineId)) {
        await setViewerMachineId(viewerId, machine.id);
      }

      setViewerMachineIdCache(machine.id);
      clearViewerDeviceName();

      setMachineId(machine.id);
      setName(machine.name);
      setIcon(machine.icon ?? '');
      setHostname(machine.hostname ?? '');
      setNameDraft(machine.name);
      setIconDraft(machine.icon ?? '');
      setHostnameDraft(machine.hostname ?? '');
      setPromptMode('closed');
      window.dispatchEvent(new Event(VIEWER_IDENTITY_CHANGED));
      return true;
    } catch {
      setError('saveFailed');
      return false;
    } finally {
      setIsSaving(false);
    }
  }, [iconDraft, hostnameDraft, machineId, nameDraft, viewerId]);

  const openPrompt = useCallback(() => {
    setNameDraft(name ?? '');
    setIconDraft(icon);
    setHostnameDraft(hostname);
    setError(null);
    setPromptMode('edit');
  }, [hostname, icon, name]);

  const closePrompt = useCallback(() => {
    if (promptMode === 'blocked' && !name) {
      return;
    }
    setPromptMode('closed');
    setError(null);
  }, [name, promptMode]);

  /**
   * Prefill hostname from the gateway OS hint when the draft is still empty.
   */
  const prefillHostnameHint = useCallback(async () => {
    if (hostnameDraft.trim()) {
      return;
    }
    try {
      const hinted = await getHostname();
      if (hinted.trim()) {
        setHostnameDraft(hinted.trim());
      }
    } catch {
      /* hostname hint is optional */
    }
  }, [hostnameDraft]);

  /**
   * Link this viewer profile to an existing machine catalog row by UUID.
   */
  const linkMachineById = useCallback(
    async (id?: string): Promise<boolean> => {
      const candidate = (id ?? linkMachineIdDraft).trim();
      if (!isMachineUuid(candidate)) {
        setLinkError('invalidId');
        return false;
      }

      setIsLinking(true);
      setLinkError(null);
      try {
        const machines = await listMachines();
        const machine = machines.find((entry) => entry.id === candidate);
        if (!machine) {
          setLinkError('linkNotFound');
          return false;
        }

        await setViewerMachineId(viewerId, candidate);
        setViewerMachineIdCache(candidate);
        clearViewerDeviceName();

        const profile = toViewerProfile(machine);
        setMachineId(normalizeMachineId(profile.id));
        setName(profile.name);
        setIcon(profile.icon);
        setHostname(profile.hostname);
        setNameDraft(profile.name);
        setIconDraft(profile.icon);
        setHostnameDraft(profile.hostname);
        setLinkMachineIdDraft('');

        window.dispatchEvent(new Event(VIEWER_IDENTITY_CHANGED));
        return true;
      } catch {
        setLinkError('linkFailed');
        return false;
      } finally {
        setIsLinking(false);
      }
    },
    [linkMachineIdDraft, viewerId],
  );

  const profileDraft = { name: nameDraft, icon: iconDraft, hostname: hostnameDraft };
  const isProfileDirty =
    !normalizeMachineId(machineId) ||
    nameDraft !== (name ?? '') ||
    iconDraft !== icon ||
    hostnameDraft !== hostname;
  const canSaveProfile =
    isMachineProfileComplete(profileDraft) && isProfileDirty;
  const showPrompt = promptMode !== 'closed';

  return {
    name,
    icon,
    machineId,
    hints,
    isLoading,
    isSaving,
    promptMode,
    showPrompt,
    nameDraft,
    iconDraft,
    hostnameDraft,
    setNameDraft,
    setIconDraft,
    setHostnameDraft,
    error,
    setError,
    saveProfile,
    canSaveProfile,
    openPrompt,
    closePrompt,
    prefillHostnameHint,
    linkMachineIdDraft,
    setLinkMachineIdDraft,
    isLinking,
    linkError,
    linkMachineById,
  };
}
