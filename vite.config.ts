import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';

/** Only @codemirror/lang-markdown gets the slim HTML language; src/markdown-html.ts says why. */
const slimHtmlForMarkdown: Plugin = {
  name: 'marky:slim-html-for-markdown',
  enforce: 'pre',
  resolveId(source, importer) {
    if (source !== '@codemirror/lang-html' || !importer?.includes('@codemirror/lang-markdown')) return null;
    return fileURLToPath(new URL('./src/markdown-html.ts', import.meta.url));
  },
};

// Tauri expects a fixed dev port and must not have its logs cleared.
export default defineConfig({
  clearScreen: false,
  plugins: [slimHtmlForMarkdown],
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
