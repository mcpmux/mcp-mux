import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist', 'src-tauri'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      '@typescript-eslint/no-unused-vars': ['warn', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-explicit-any': 'warn',
      // The transport facade (src/lib/transport) is the single seam to the
      // backend so the same UI runs in Tauri and in the web admin. Reach for
      // `call`/`subscribe` from '@/lib/transport', never raw Tauri IPC.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              message:
                "Import { call } from '@/lib/transport' instead of raw Tauri invoke (keeps the UI transport-independent).",
            },
          ],
        },
      ],
    },
  },
  {
    // The Tauri transport is the ONE place allowed to touch raw IPC.
    files: ['src/lib/transport/tauri.ts'],
    rules: { 'no-restricted-imports': 'off' },
  }
);
