import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import { cfAccessHeadersFromEnv, loadRepoDotEnv } from '../../scripts/cf-access-env.mjs';

loadRepoDotEnv(path.resolve(__dirname, '../..'));

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process is a nodejs global
// Admin HTTP transport: explicit VITE_ADMIN_WEB, or dev:admin / dev:web:admin sessions.
const isAdminWeb =
  Boolean(process.env.VITE_ADMIN_WEB) || process.env.MCPMUX_DEV_ADMIN === '1';
// @ts-expect-error process is a nodejs global
const adminPort = Number.parseInt(process.env.MCPMUX_ADMIN_PORT ?? '45819', 10);

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Path aliases
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
      '@mcpmux/ui': path.resolve(__dirname, '../../packages/ui/src'),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
    ...(isAdminWeb
      ? {
          proxy: {
            '/api': {
              target: `http://127.0.0.1:${adminPort}`,
              changeOrigin: true,
              headers: cfAccessHeadersFromEnv(),
            },
          },
        }
      : {}),
  },
}));
