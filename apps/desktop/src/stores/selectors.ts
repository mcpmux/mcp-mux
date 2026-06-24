import { useAppStore } from './appStore';
import { Space } from '@/lib/api/spaces';

// Typed selectors for better performance
export const useSpaces = () => useAppStore((state) => state.spaces);
export const useViewSpaceId = () => useAppStore((state) => state.viewSpaceId);
export const useActiveNav = () => useAppStore((state) => state.activeNav);
export const useNavigateTo = () => useAppStore((state) => state.navigateTo);
export const usePendingClientId = () => useAppStore((state) => state.pendingClientId);
export const useSetPendingClientId = () => useAppStore((state) => state.setPendingClientId);
export const usePendingSettingsSection = () =>
  useAppStore((state) => state.pendingSettingsSection);
export const useSetPendingSettingsSection = () =>
  useAppStore((state) => state.setPendingSettingsSection);
export const useTheme = () => useAppStore((state) => state.theme);
export const useSidebarCollapsed = () => useAppStore((state) => state.sidebarCollapsed);
export const useAnalyticsEnabled = () => useAppStore((state) => state.analyticsEnabled);

// Computed selectors
export const useViewSpace = (): Space | null => {
  const spaces = useSpaces();
  const viewSpaceId = useViewSpaceId();
  return spaces.find((s) => s.id === viewSpaceId) ?? null;
};

/** The system's fallback space — `is_default` Space, used by gateway when no WorkspaceBinding matches. */
export const useDefaultSpace = (): Space | null => {
  const spaces = useSpaces();
  return spaces.find((s) => s.is_default) ?? null;
};

export const useIsLoading = (key: 'spaces' | 'servers') => {
  return useAppStore((state) => state.loading[key]);
};
