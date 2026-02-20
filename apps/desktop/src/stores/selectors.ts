import { useAppStore } from './appStore';
import { Space } from '@/lib/api/spaces';

// Typed selectors for better performance
export const useSpaces = () => useAppStore((state) => state.spaces);
export const useActiveSpaceId = () => useAppStore((state) => state.activeSpaceId);
export const useViewSpaceId = () => useAppStore((state) => state.viewSpaceId);
export const useTheme = () => useAppStore((state) => state.theme);
export const useSidebarCollapsed = () => useAppStore((state) => state.sidebarCollapsed);
export const useAnalyticsEnabled = () => useAppStore((state) => state.analyticsEnabled);

// Computed selectors
export const useActiveSpace = (): Space | null => {
  const spaces = useSpaces();
  const activeSpaceId = useActiveSpaceId();
  return spaces.find((s) => s.id === activeSpaceId) ?? null;
};

export const useViewSpace = (): Space | null => {
  const spaces = useSpaces();
  const activeSpaceId = useActiveSpaceId();
  const viewSpaceId = useViewSpaceId();
  const effectiveId = viewSpaceId ?? activeSpaceId;
  return spaces.find((s) => s.id === effectiveId) ?? null;
};

export const useIsLoading = (key: 'spaces' | 'servers') => {
  return useAppStore((state) => state.loading[key]);
};

