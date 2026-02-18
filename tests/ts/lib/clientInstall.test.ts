import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../../apps/desktop/src/lib/api/clientInstall', () => ({
  addToVscode: vi.fn(),
  addToCursor: vi.fn(),
}));

import {
  addToVscode,
  addToCursor,
} from '../../../apps/desktop/src/lib/api/clientInstall';

const mockedAddVscode = vi.mocked(addToVscode);
const mockedAddCursor = vi.mocked(addToCursor);

describe('clientInstall API', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('addToVscode returns void on success', async () => {
    mockedAddVscode.mockResolvedValue(undefined);
    await expect(addToVscode('http://localhost:45818')).resolves.toBeUndefined();
  });

  it('addToCursor returns void on success', async () => {
    mockedAddCursor.mockResolvedValue(undefined);
    await expect(addToCursor('http://localhost:45818')).resolves.toBeUndefined();
  });

  it('addToVscode rejects on error', async () => {
    mockedAddVscode.mockRejectedValue(new Error('VS Code not found'));
    await expect(addToVscode('http://localhost:45818')).rejects.toThrow('VS Code not found');
  });

  it('addToCursor rejects on error', async () => {
    mockedAddCursor.mockRejectedValue(new Error('Cursor not found'));
    await expect(addToCursor('http://localhost:45818')).rejects.toThrow('Cursor not found');
  });
});
