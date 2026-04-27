import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { TRANSPORT_MODE } from './api/transport';

// Tauri-only — pipes Rust tracing logs into the browser devtools. The plugin
// crashes on import in browser mode (no `window.__TAURI__`), so gate it.
if (TRANSPORT_MODE === 'tauri') {
  void import('@tauri-apps/plugin-log').then((m) => m.attachConsole());
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
