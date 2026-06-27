import { create } from 'zustand';
import { immer } from 'zustand/middleware/immer';
import type { WorkspaceBinding } from '@/lib/api/workspaceBindings';

/** How the unified binding panel was opened. */
export type BindingPanelMode = 'create' | 'create-from-live' | 'edit';

/** Ephemeral open payload for the workspace binding panel. */
export interface BindingPanelPayload {
  mode: BindingPanelMode;
  /** Prefilled workspace root for create-from-live. */
  workspaceRoot?: string;
  /** Existing binding when editing. */
  binding?: WorkspaceBinding;
  /** OAuth client id from workspace-needs-binding. */
  clientId?: string;
  /** Hint for default space picker from the triggering event. */
  spaceId?: string;
  /** When set, header shows collision copy instead of new-connection badge. */
  collisionClientId?: string;
}

interface BindingPanelStore {
  isOpen: boolean;
  payload: BindingPanelPayload | null;
  open: (payload: BindingPanelPayload) => void;
  close: () => void;
}

/**
 * Ephemeral UI state for the global workspace binding panel overlay.
 * Not persisted — open/close and payload reset on each session.
 */
export const useBindingPanelStore = create<BindingPanelStore>()(
  immer((set) => ({
    isOpen: false,
    payload: null,

    open: (payload) =>
      set((state) => {
        state.isOpen = true;
        state.payload = payload;
      }),

    close: () =>
      set((state) => {
        state.isOpen = false;
        state.payload = null;
      }),
  }))
);
