import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import type { EditorState } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';

export interface Heading {
  level: number;
  text: string;
  /** 0-based source line, the same unit as the preview's data-line. */
  line: number;
}

const HEADING_NODE = /^(ATXHeading|SetextHeading)([1-6])$/;
/** How long one refresh may spend parsing below the viewport; the rest follows on the next pause. */
const PARSE_BUDGET_MS = 50;

/**
 * Headings from the Markdown syntax tree (PRD F17), so `#` lines inside code
 * fences or frontmatter never show up.
 */
export function extractHeadings(state: EditorState): Heading[] {
  const tree = ensureSyntaxTree(state, state.doc.length, PARSE_BUDGET_MS) ?? syntaxTree(state);
  const headings: Heading[] = [];
  tree.iterate({
    enter(node) {
      const match = HEADING_NODE.exec(node.name);
      if (!match) return;
      const line = state.doc.lineAt(node.from);
      const text = headingText(line.text, match[1] === 'ATXHeading');
      if (text) headings.push({ level: Number(match[2]), text, line: line.number - 1 });
      return false;
    },
  });
  return headings;
}

/** Heading text without `#` markers and the most common inline markup. */
function headingText(line: string, atx: boolean): string {
  const raw = atx ? line.replace(/^\s{0,3}#{1,6}\s*/, '').replace(/\s+#+\s*$/, '') : line;
  return raw
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_`~]/g, '')
    .trim();
}

/** The collapsible heading list left of the editor; click a heading to jump to it. */
export function createOutline(el: HTMLElement, view: EditorView, onSelect: (line: number) => void) {
  let headings: Heading[] = [];

  el.addEventListener('click', (e) => {
    const button = (e.target as Element).closest<HTMLButtonElement>('button[data-line]');
    if (button) onSelect(Number(button.dataset.line));
  });

  /** Follows the cursor, not the scroll position: the last headings of a short file never reach the top. */
  function highlightCurrent() {
    const cursorLine = view.state.doc.lineAt(view.state.selection.main.head).number - 1;
    let current = -1;
    headings.forEach((h, i) => {
      if (h.line <= cursorLine) current = i;
    });
    el.querySelectorAll('button[data-line]').forEach((button, i) => {
      button.toggleAttribute('aria-current', i === current);
    });
  }

  function render() {
    if (headings.length === 0) {
      const empty = document.createElement('p');
      empty.className = 'outline-empty';
      empty.textContent = 'No headings';
      el.replaceChildren(empty);
      return;
    }
    const list = document.createElement('ul');
    for (const heading of headings) {
      const button = document.createElement('button');
      button.dataset.line = String(heading.line);
      button.textContent = heading.text;
      button.title = heading.text;
      button.style.paddingLeft = `${8 + (heading.level - 1) * 12}px`;
      const item = document.createElement('li');
      item.append(button);
      list.append(item);
    }
    el.replaceChildren(list);
    highlightCurrent();
  }

  return {
    /** Call on cursor moves; cheap, and a no-op while hidden. */
    highlightCurrent() {
      if (isVisible()) highlightCurrent();
    },
    /** Re-reads headings from the editor; a no-op while hidden, so a closed outline costs nothing. */
    refresh() {
      if (isVisible()) reload();
    },
    setVisible,
  };

  function isVisible(): boolean {
    return !el.hidden;
  }

  function setVisible(visible: boolean) {
    el.hidden = !visible;
    if (visible) reload();
  }

  function reload() {
    headings = extractHeadings(view.state);
    render();
  }
}

