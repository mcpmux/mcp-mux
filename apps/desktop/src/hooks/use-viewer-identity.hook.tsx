import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import {
  createMachine,
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
  const [showPrompt, setShowPrompt] = useState(false);
  const [nameDraft, setNameDraft] = useState('');
  const [iconDraft, setIconDraft] = useState('');
  const [hostnameDraft, setHostnameDraft] = useState('');
  const [error, setError] = useState<string | null>(null);

  const applyMachine = useCallback((machine: ViewerMachineProfile | null) => {
    setMachineId(machine?.id ?? null);
    setName(machine?.name ?? null);
    setIcon(machine?.icon ?? '');
    setHostname(machine?.hostname ?? '');
    setNameDraft(machine?.name ?? '');
    setIconDraft(machine?.icon ?? '');
    setHostnameDraft(machine?.hostname ?? '');
    const isComplete =
      machine != null &&
      isMachineProfileComplete({
        name: machine.name,
        icon: machine.icon,
        hostname: machine.hostname,
      });
    setShowPrompt(!isComplete);
  }, []);

  const refreshIdentity = useCallback(async () => {
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
          applyMachine({ id: '', name: legacyName, icon: '', hostname: '' });
          return;
        }
      }

      applyMachine(machine);
    } catch {
      const legacyName = getViewerDeviceName();
      applyMachine(
        legacyName ? { id: '', name: legacyName, icon: '', hostname: '' } : null,
      );
    } finally {
      setIsLoading(false);
    }
  }, [applyMachine, viewerId]);

  useEffect(() => {
    void getViewerDeviceHints().then(setHints);
    void refreshIdentity();

    const onChanged = () => {
      void refreshIdentity();
    };
    window.addEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
    return () => window.removeEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
  }, [refreshIdentity]);

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
      const machine = machineId
        ? await updateMachine(machineId, payload)
        : await createMachine(payload);

      if (viewingLocally) {
        if (!machineId) {
          await setLocalMachineId(machine.id);
        }
        await setViewerMachineId(viewerId, null);
      } else if (!machineId) {
        await setViewerMachineId(viewerId, machine.id);
      }

      setViewerMachineIdCache(machine.id);
      clearViewerDeviceName();

      setMachineId(machine.id);
      setName(machine.name);
      setIcon(machine.icon ?? '');
      setHostname(machine.hostname ?? '');
      setShowPrompt(false);
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
    setShowPrompt(true);
  }, [hostname, icon, name]);

  const closePrompt = useCallback(() => {
    if (!name) {
      return;
    }
    setShowPrompt(false);
    setError(null);
  }, [name]);

  const profileDraft = { name: nameDraft, icon: iconDraft, hostname: hostnameDraft };
  const isProfileDirty =
    !machineId ||
    nameDraft !== (name ?? '') ||
    iconDraft !== icon ||
    hostnameDraft !== hostname;
  const canSaveProfile =
    isMachineProfileComplete(profileDraft) && isProfileDirty;

  return {
    name,
    icon,
    machineId,
    hints,
    isLoading,
    isSaving,
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
  };
}
