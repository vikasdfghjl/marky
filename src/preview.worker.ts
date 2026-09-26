import MarkdownIt from 'markdown-it';
import taskLists from 'markdown-it-task-lists';

export interface RenderRequest {
  id: number;
  text: string;
}

export interface RenderResult {
  id: number;
  html: string;
  ms: number;
}

// html: false renders raw HTML as text, so untrusted files cannot inject markup.
const md = new MarkdownIt({ html: false, linkify: true }).use(taskLists, { enabled: false });

// Tag block elements with their source line for synced scroll (F10).
md.core.ruler.push('source_lines', (state) => {
  for (const tok of state.tokens) {
    if (tok.map && tok.nesting === 1) tok.attrSet('data-line', String(tok.map[0]));
  }
});

self.onmessage = (e: MessageEvent<RenderRequest>) => {
  const start = performance.now();
  const html = md.render(e.data.text);
  const result: RenderResult = { id: e.data.id, html, ms: performance.now() - start };
  self.postMessage(result);
};
