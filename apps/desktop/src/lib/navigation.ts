/**
 * Navigation model — the single source of truth for the sidebar.
 *
 * The app's IA follows the superapp plan (mcpmux.space/superapp/03-experience-design.md):
 * a "use" zone on top (today just Home; Chat and Agents land here later), a
 * "Library" zone for capabilities (Tools, Context, Discover; Models lands here
 * later), and a "Control" zone for routing & access (Apps, Workspaces,
 * FeatureSets, Spaces). Settings is pinned to the sidebar footer.
 *
 * To add a future surface, append an entry to the right zone — the sidebar
 * renders from this data and nothing else.
 *
 * NOTE: `key` values are NavItem store keys and `testId`s are the e2e selector
 * contract (ADR-003) — both are stable identifiers. Only `label`/`icon`/`hint`
 * are presentation and safe to change.
 */
import type { LucideIcon } from 'lucide-react';
import {
  Home,
  Server,
  Sparkles,
  Compass,
  Monitor,
  FolderOpen,
  Wrench,
  Globe,
  Settings,
} from 'lucide-react';
import type { NavItem } from '@/stores/types';

export interface NavEntry {
  key: NavItem;
  label: string;
  icon: LucideIcon;
  testId: string;
  /** One-line tooltip so newcomers learn the map by hovering. */
  hint: string;
}

export interface NavZone {
  /** Zone label; omitted for the top-level "use" zone. */
  title?: string;
  entries: NavEntry[];
}

export const NAV_ZONES: NavZone[] = [
  {
    entries: [
      {
        key: 'home',
        label: 'Home',
        icon: Home,
        testId: 'nav-dashboard',
        hint: 'Your gateway, connections, and Space at a glance',
      },
    ],
  },
  {
    title: 'Library',
    entries: [
      {
        key: 'servers',
        label: 'Tools',
        icon: Server,
        testId: 'nav-my-servers',
        hint: 'MCP servers installed in this Space',
      },
      {
        key: 'builtin-servers',
        label: 'Built-in',
        icon: Sparkles,
        testId: 'nav-builtin-servers',
        hint: 'Capabilities McpMux itself provides — self-management now; memory & skills next',
      },
      {
        key: 'registry',
        label: 'Discover',
        icon: Compass,
        testId: 'nav-discover',
        hint: 'Browse and install servers from the registry',
      },
    ],
  },
  {
    title: 'Control',
    entries: [
      {
        key: 'clients',
        label: 'Apps',
        icon: Monitor,
        testId: 'nav-clients',
        hint: 'AI apps connected through your gateway',
      },
      {
        key: 'workspaces',
        label: 'Workspaces',
        icon: FolderOpen,
        testId: 'nav-workspaces',
        hint: 'Folder → tools mappings',
      },
      {
        key: 'featuresets',
        label: 'FeatureSets',
        icon: Wrench,
        testId: 'nav-featuresets',
        hint: 'Curated tool bundles you can grant and map',
      },
      {
        key: 'spaces',
        label: 'Spaces',
        icon: Globe,
        testId: 'nav-spaces',
        hint: 'Isolated contexts — work, personal, per client',
      },
    ],
  },
];

/** Pinned to the sidebar footer, below the scrolling zones. */
export const NAV_SETTINGS: NavEntry = {
  key: 'settings',
  label: 'Settings',
  icon: Settings,
  testId: 'nav-settings',
  hint: 'Preferences, gateway, and updates',
};
