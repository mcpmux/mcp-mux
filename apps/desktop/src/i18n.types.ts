import 'i18next';
import type clients from './locales/en/clients.json';
import type common from './locales/en/common.json';
import type dashboard from './locales/en/dashboard.json';
import type metatools from './locales/en/metatools.json';
import type featuresets from './locales/en/featuresets.json';
import type home from './locales/en/home.json';
import type nav from './locales/en/nav.json';
import type registry from './locales/en/registry.json';
import type servers from './locales/en/servers.json';
import type settings from './locales/en/settings.json';
import type spaces from './locales/en/spaces.json';
import type workspaces from './locales/en/workspaces.json';

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common';
    resources: {
      nav: typeof nav;
      common: typeof common;
      dashboard: typeof dashboard;
      servers: typeof servers;
      workspaces: typeof workspaces;
      featuresets: typeof featuresets;
      clients: typeof clients;
      settings: typeof settings;
      spaces: typeof spaces;
      registry: typeof registry;
      metatools: typeof metatools;
      home: typeof home;
    };
  }
}
