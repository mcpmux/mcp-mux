// API layer for communicating with Tauri backend

export * from './app';
export * from './spaces';
export * from './registry';
export * from './featureSets';
export * from './serverFeatures';
export * from './clientInstall';
export * from './clients';
export * from './gateway';
export * from './serverManager';
export * from './workspaceBindings';
export * from './metaTools';
export * from './configExport';
export type {
  ConsentRequestDetails,
  ConsentError,
  ConsentApprovalRequest,
  ConsentApprovalResponse,
} from './oauth';
export { flushPendingDeepLink, getPendingConsent, approveOAuthConsent } from './oauth';
export * from './serverClone';
export * from './settings';
export * from './workspaceAppearances';
export * from './logs';
