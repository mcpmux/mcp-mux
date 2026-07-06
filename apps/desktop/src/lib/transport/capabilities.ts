/**
 * Platform capabilities a transport exposes. Web mode (the headless gateway's
 * admin UI) can't reach OS-only affordances the Tauri shell provides, so
 * components gate on these instead of forking by build.
 */
export interface Capabilities {
  /** Native file/folder dialogs (plugin-dialog). */
  dialog: boolean;
  /** Writing files into the user's projects/config (WorkspaceInstallPanel). */
  fsWrite: boolean;
  /** System tray affordances. */
  tray: boolean;
  /** `mcpmux://` deep-link handling. */
  deepLink: boolean;
  /** Launch-at-login / autostart settings. */
  autostart: boolean;
  /** In-app updater. */
  updater: boolean;
}

/** Everything available — the desktop (Tauri) shell. */
export const DESKTOP_CAPABILITIES: Capabilities = {
  dialog: true,
  fsWrite: true,
  tray: true,
  deepLink: true,
  autostart: true,
  updater: true,
};

/** Browser/web-admin: only what a plain web page can do. */
export const WEB_CAPABILITIES: Capabilities = {
  dialog: false,
  fsWrite: false,
  tray: false,
  deepLink: false,
  autostart: false,
  updater: false,
};
