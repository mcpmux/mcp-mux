/**
 * `looksLikeFolderPath` decides whether a routing key is a folder (path
 * mapping) or an id/label (id mapping) — e.g. a value a headless client sends
 * in the `X-Mcpmux-Workspace` header. Misclassifying an id as a folder makes
 * saving its mapping fail path validation.
 */

import { describe, it, expect } from 'vitest';
import { looksLikeFolderPath } from '@/lib/api/workspaceBindings';

describe('looksLikeFolderPath', () => {
  it.each([
    '/home/me/proj',
    'C:\\work\\proj',
    'd:/work/proj',
    '\\\\server\\share\\proj',
    '//server/share/proj',
    'file:///home/me/proj',
    '  /home/me/proj  ',
  ])('treats %s as a folder', (key) => {
    expect(looksLikeFolderPath(key)).toBe(true);
  });

  it.each(['mcp_ab12cd34', 'ci-runner', 'my-laptop', 'prod.bot'])('treats %s as an id', (key) => {
    expect(looksLikeFolderPath(key)).toBe(false);
  });
});
