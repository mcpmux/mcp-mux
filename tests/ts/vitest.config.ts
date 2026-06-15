import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: [path.resolve(__dirname, 'setup.ts')],
    include: ['**/*.test.{ts,tsx}'],
    exclude: ['**/node_modules/**'],
    root: __dirname,
    reporters: ['default', 'junit'],
    outputFile: {
      junit: './test-results/vitest-junit.xml',
    },
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      reportsDirectory: './coverage',
      exclude: [
        'node_modules/',
        'setup.ts',
        '**/*.d.ts',
        'fixtures/**',
      ],
    },
    testTimeout: 10000,
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, '../../apps/desktop/src'),
      '@mcpmux/ui': path.resolve(__dirname, '../../packages/ui/src'),
      // Tauri packages live in apps/desktop/node_modules — alias them so
      // vi.mock() calls in tests resolve to the same module IDs as the
      // source code imports from apps/desktop/src/.
      '@tauri-apps/api': path.resolve(__dirname, '../../apps/desktop/node_modules/@tauri-apps/api'),
      '@tauri-apps/plugin-updater': path.resolve(__dirname, '../../apps/desktop/node_modules/@tauri-apps/plugin-updater'),
      '@tauri-apps/plugin-process': path.resolve(__dirname, '../../apps/desktop/node_modules/@tauri-apps/plugin-process'),
      '@tauri-apps/plugin-opener': path.resolve(__dirname, '../../apps/desktop/node_modules/@tauri-apps/plugin-opener'),
    },
  },
});
