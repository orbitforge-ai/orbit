import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * Reads ~/.orbit/dev_token (written by the Rust shim on first run) and exposes
 * it as `VITE_DEV_TOKEN` so the browser-mode transport can authenticate
 * against the loopback shim. Re-reads on every Vite restart but not on HMR;
 * if the token is missing we leave the env unset and the transport will
 * surface a clear 401 from the shim.
 */
function devTokenPlugin(): Plugin {
  // @ts-expect-error process is a nodejs global
  const dataDir = process.env.ORBIT_DATA_DIR || path.join(os.homedir(), '.orbit');
  const tokenPath = path.join(dataDir, 'dev_token');
  return {
    name: 'orbit-dev-token',
    config() {
      try {
        const token = fs.readFileSync(tokenPath, 'utf8').trim();
        return {
          define: {
            'import.meta.env.VITE_DEV_TOKEN': JSON.stringify(token),
          },
        };
      } catch {
        // First run before the desktop app has written the token, or running
        // against a non-shim backend. Silent — the transport reports auth
        // failures itself if mode === 'http'.
        return {};
      }
    },
  };
}

export default defineConfig(async () => ({
  plugins: [tailwindcss(), react(), devTokenPlugin()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
    proxy: {
      // The shim binds on 127.0.0.1:8765 (see src-tauri/src/shim/router.rs).
      // Browser-mode dev hits these paths, Tauri-mode ignores them.
      '/rpc': 'http://127.0.0.1:8765',
      '/ws': { target: 'ws://127.0.0.1:8765', ws: true },
    },
  },
}));
