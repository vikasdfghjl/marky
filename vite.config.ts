import { defineConfig } from 'vite';

// Tauri expects a fixed dev port and must not have its logs cleared.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // WebView2 is evergreen Chromium.
    target: 'chrome120',
    // Workers are emitted as separate files (never blob: URLs) so the CSP can stay at worker-src 'self'.
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  worker: {
    format: 'es',
  },
});
