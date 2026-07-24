#!/usr/bin/env node
/**
 * Fail when TSX under apps/desktop/src uses hardcoded English `title="…"` attributes.
 * Product copy should come from i18n JSON; dynamic titles may use title={t(...)}.
 */

import { execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TARGET = 'apps/desktop/src';

try {
  const out = execSync(`rg 'title="[A-Z]' --glob '*.tsx' ${TARGET}`, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  });
  if (out.trim()) {
    console.error('[lint:i18n] Hardcoded title= attributes (use i18n or title={…}):\n');
    console.error(out);
    process.exit(1);
  }
} catch (error) {
  const status = /** @type {NodeJS.ErrnoException & { status?: number }} */ (error).status;
  if (status === 1) {
    process.exit(0);
  }
  throw error;
}
