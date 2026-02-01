import React from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import App from './App';
import './index.css';

// Expose Tauri API for E2E testing
// This allows tests to set up data programmatically
declare global {
  interface Window {
    __TAURI_TEST_API__?: {
      invoke: typeof invoke;
    };
  }
}

// Always expose for now - can be gated by env var if needed
window.__TAURI_TEST_API__ = { invoke };

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
