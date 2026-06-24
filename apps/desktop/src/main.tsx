import React from 'react';
import ReactDOM from 'react-dom/client';
import './i18n';
import { logWebAdminBuildInfo } from '@/lib/build-info.helpers';
import { initTauriTestApi } from '@/lib/backend/shell';
import '@/lib/monaco-setup';
import App from './App';
import './index.css';

/**
 * Boot the SPA after the web-admin build banner finishes (keeps console.group unbroken).
 */
async function bootstrap(): Promise<void> {
  initTauriTestApi();
  await logWebAdminBuildInfo();

  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void bootstrap();
