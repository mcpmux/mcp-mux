/**
 * Screenshot Preseed Data
 *
 * Single source of truth for all mock data used in screenshot capture.
 * Edit this file to change what appears in screenshots, then re-run:
 *   pnpm exec wdio run tests/e2e/wdio.conf.ts --spec tests/e2e/specs/capture-screenshots.manual.ts
 */

export const PRESEED: {
  spaces: { name: string; icon: string }[];
  serversToInstall: string[];
  featureSets: { name: string; description: string; icon?: string }[];
} = {
  /** Additional spaces to create (default space is created automatically) */
  spaces: [
    { name: 'Work Projects', icon: '💼' },
    { name: 'Personal', icon: '🏠' },
    { name: 'Experiments', icon: '🧪' },
    { name: 'Production', icon: '🚀' },
    { name: 'Staging', icon: '🔧' },
  ],

  /** Server IDs to install in the default space (must match IDs in fixtures.ts).
   *  Order matters — GitHub first for screenshot prominence. */
  serversToInstall: [
    'github-server',
    'filesystem-server',
    'postgres-server',
    'slack-server',
    'brave-search',
    'docker-server',
    'notion-server',
    'aws-server',
    'cloudflare-workers-server',
    'azure-server',
  ],

  /** Custom feature sets to create in the default space */
  featureSets: [
    { name: 'Read Only', description: 'Only read operations — no writes or deletes', icon: '🔒' },
    { name: 'Dev Tools', description: 'GitHub + PostgreSQL + Filesystem access', icon: '🛠️' },
    { name: 'Full Access', description: 'All servers and capabilities enabled', icon: '🚀' },
  ],
};
