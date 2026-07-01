import { useLocation } from 'wouter';
import { NAV_PATH_MAP } from '@/lib/navigation';
import type { NavItem } from '@/stores/types';

/**
 * Navigate to a top-level app page by NavItem key, with optional URL search params.
 */
export function useNavigate() {
  const [, navigate] = useLocation();

  return (nav: NavItem, query?: Record<string, string>) => {
    const path = NAV_PATH_MAP[nav];
    if (!query || Object.keys(query).length === 0) {
      navigate(path);
      return;
    }
    navigate(`${path}?${new URLSearchParams(query).toString()}`);
  };
}
