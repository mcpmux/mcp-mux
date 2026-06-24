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
 * contract (ADR-003) — both are stable identifiers. Only `labelKey`/`hintKey`/
 * `icon` are presentation and safe to change.
 */
import type { LucideIcon } from 'lucide-react';
import {
  Home,
  LayoutDashboard,
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
import type nav from '@/locales/en/nav.json';

/** Top-level nav label keys (excludes nested `zones` / `hints` objects). */
type NavLabelKey = Exclude<keyof typeof nav, 'zones' | 'hints'>;

/** Sidebar hint keys under `nav.hints`. */
type NavHintKey = `hints.${keyof typeof nav.hints}`;

/** Zone section title keys under `nav.zones`. */
type NavZoneTitleKey = `zones.${keyof typeof nav.zones}`;

export interface NavEntry {
  key: NavItem;
  /** i18n key under the `nav` namespace (e.g. `home` → nav:home). */
  labelKey: NavLabelKey;
  icon: LucideIcon;
  testId: string;
  /** i18n key under nav:hints.* */
  hintKey: NavHintKey;
}

export interface NavZone {
  /** Zone label i18n key under nav:zones.*; omitted for the top-level "use" zone. */
  titleKey?: NavZoneTitleKey;
  entries: NavEntry[];
}

export const NAV_ZONES: NavZone[] = [
  {
    entries: [
      {
        key: 'home',
        labelKey: 'home',
        icon: Home,
        testId: 'nav-home',
        hintKey: 'hints.home',
      },
      {
        key: 'dashboard',
        labelKey: 'dashboard',
        icon: LayoutDashboard,
        testId: 'nav-dashboard',
        hintKey: 'hints.dashboard',
      },
    ],
  },
  {
    titleKey: 'zones.library',
    entries: [
      {
        key: 'servers',
        labelKey: 'tools',
        icon: Server,
        testId: 'nav-my-servers',
        hintKey: 'hints.tools',
      },
      {
        key: 'builtin-servers',
        labelKey: 'builtin',
        icon: Sparkles,
        testId: 'nav-builtin-servers',
        hintKey: 'hints.builtin',
      },
      {
        key: 'registry',
        labelKey: 'discover',
        icon: Compass,
        testId: 'nav-discover',
        hintKey: 'hints.discover',
      },
    ],
  },
  {
    titleKey: 'zones.control',
    entries: [
      {
        key: 'clients',
        labelKey: 'apps',
        icon: Monitor,
        testId: 'nav-clients',
        hintKey: 'hints.apps',
      },
      {
        key: 'workspaces',
        labelKey: 'workspaces',
        icon: FolderOpen,
        testId: 'nav-workspaces',
        hintKey: 'hints.workspaces',
      },
      {
        key: 'featuresets',
        labelKey: 'featuresets',
        icon: Wrench,
        testId: 'nav-featuresets',
        hintKey: 'hints.featuresets',
      },
      {
        key: 'spaces',
        labelKey: 'spaces',
        icon: Globe,
        testId: 'nav-spaces',
        hintKey: 'hints.spaces',
      },
    ],
  },
];

/** Pinned to the sidebar footer, below the scrolling zones. */
export const NAV_SETTINGS: NavEntry = {
  key: 'settings',
  labelKey: 'settings',
  icon: Settings,
  testId: 'nav-settings',
  hintKey: 'hints.settings',
};
