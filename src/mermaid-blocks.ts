import type { Mermaid } from 'mermaid';
import { isDark, onThemeChange } from './theme';

// Draws ```mermaid fences (PRD F16). Mermaid is about 1 MB, so it loads only when a
// document contains a diagram, and each SVG is cached by source and theme: the preview
// re-renders on every typing pause, but most diagrams have not changed since last time.

const CACHE_LIMIT = 50;
const svgCache = new Map<string, string>();
let mermaid: Promise<Mermaid> | null = null;
let renderCount = 0;

/** 'strict' encodes HTML in labels and disables click handlers: the source is untrusted. */
function configure(m: Mermaid): Mermaid {
  m.initialize({ startOnLoad: false, securityLevel: 'strict', theme: isDark() ? 'dark' : 'default' });
  return m;
}

function loadMermaid(): Promise<Mermaid> {
  mermaid ??= import('mermaid').then(({ default: m }) => configure(m));
  return mermaid;
}

/** Re-renders diagrams when the app theme flips (the cache is keyed by theme). */
export function onMermaidThemeChange(rerender: () => void) {
  onThemeChange(() => {
    if (!mermaid) return;
    void mermaid.then((m) => {
      configure(m);
      rerender();
    });
  });
}

/**
 * Replaces each `.mermaid` placeholder in `container` with its SVG, or an inline error.
 * `isCurrent` lets a newer preview render abandon this one mid-way.
 */
export async function drawMermaidBlocks(container: HTMLElement, isCurrent: () => boolean) {
  const blocks = [...container.querySelectorAll<HTMLElement>('.mermaid:not([data-drawn])')];
  if (blocks.length === 0) return;
  const theme = isDark() ? 'dark' : 'default';

  for (const block of blocks) {
    const source = block.textContent ?? '';
    const key = `${theme}\n${source}`;
    const cached = svgCache.get(key);
    if (cached) {
      showSvg(block, cached);
      continue;
    }
    const m = await loadMermaid();
    if (!isCurrent()) return;
    await drawOne(m, block, source, key);
    if (!isCurrent()) return;
  }
}

async function drawOne(m: Mermaid, block: HTMLElement, source: string, key: string) {
  const id = `mermaid-${++renderCount}`;
  try {
    const { svg } = await m.render(id, source);
    remember(key, svg);
    showSvg(block, svg);
  } catch (err) {
    block.dataset.drawn = 'error';
    block.classList.add('mermaid-error');
    block.textContent = `Mermaid: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    // On a syntax error Mermaid leaves its scratch element in <body>.
    document.getElementById(`d${id}`)?.remove();
  }
}

function showSvg(block: HTMLElement, svg: string) {
  block.dataset.drawn = 'svg';
  block.innerHTML = svg;
}

function remember(key: string, svg: string) {
  if (svgCache.size >= CACHE_LIMIT) svgCache.delete(svgCache.keys().next().value!);
  svgCache.set(key, svg);
}
