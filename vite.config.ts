import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';

/**
 * @codemirror/lang-markdown imports @codemirror/lang-html just to highlight inline HTML tags,
 * and that package drags the JavaScript and CSS parsers into the startup bundle. Give Markdown
 * a slim HTML language instead; every other importer still gets the real package.
 */
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
