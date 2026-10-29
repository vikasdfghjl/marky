import { Compartment, EditorState, type Extension } from '@codemirror/state';
import {
  EditorView,
  keymap,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightSpecialChars,
  rectangularSelection,
  type ViewUpdate,
} from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab, redo, undo } from '@codemirror/commands';
import { HighlightStyle, bracketMatching, indentOnInput, syntaxHighlighting } from '@codemirror/language';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { yamlFrontmatter } from '@codemirror/lang-yaml';
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from '@codemirror/search';
import { tags as t } from '@lezer/highlight';
import { markdownInlineSyntax, markdownTags } from './markdown-inline-syntax';

// Colours come from CSS variables so the OS light/dark switch needs no JS.
const highlight = HighlightStyle.define([
  { tag: t.heading, fontWeight: '700', color: 'var(--hl-heading)' },
  { tag: t.strong, fontWeight: '700' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.strikethrough, textDecoration: 'line-through' },
  // Shifted with relative positioning, not vertical-align, which would grow the line box and
  // make line heights jump as you type.
  { tag: markdownTags.subscript, fontSize: '80%', position: 'relative', top: '0.25em' },
  { tag: markdownTags.superscript, fontSize: '80%', position: 'relative', top: '-0.4em' },
  { tag: markdownTags.marked, backgroundColor: 'var(--mark)' },
  { tag: [t.link, t.url], color: 'var(--hl-link)' },
  { tag: t.monospace, color: 'var(--hl-code)' },
  { tag: t.quote, color: 'var(--hl-quote)' },
  { tag: [t.processingInstruction, t.contentSeparator, t.meta], color: 'var(--hl-muted)' },
  { tag: t.keyword, color: 'var(--hl-keyword)' },
  { tag: [t.string, t.special(t.string), t.regexp], color: 'var(--hl-string)' },
  { tag: [t.number, t.bool, t.null, t.atom], color: 'var(--hl-number)' },
  { tag: [t.comment, t.lineComment, t.blockComment], color: 'var(--hl-comment)', fontStyle: 'italic' },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: 'var(--hl-fn)' },
  { tag: [t.typeName, t.className, t.tagName], color: 'var(--hl-type)' },
  { tag: [t.propertyName, t.attributeName], color: 'var(--hl-prop)' },
]);

const theme = EditorView.theme({
  '&': { height: '100%', color: 'var(--fg)', backgroundColor: 'var(--bg)' },
  '&.cm-focused': { outline: 'none' },
  // Font size is a CSS variable (settings), so it applies to every tab's state at once.
  '.cm-scroller': { fontFamily: 'var(--font-mono)', fontSize: 'var(--editor-font-size, 15px)', lineHeight: '1.6', padding: '16px 0' },
  '.cm-content': { caretColor: 'var(--fg)', padding: '0 24px', maxWidth: '100ch' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--fg)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground':
    { backgroundColor: 'var(--selection)' },
  '.cm-activeLine': { backgroundColor: 'var(--active-line)' },
  '.cm-selectionMatch': { backgroundColor: 'var(--match)' },
  '.cm-searchMatch': { backgroundColor: 'var(--match)', outline: '1px solid var(--border)' },
  '.cm-panels': { backgroundColor: 'var(--panel)', color: 'var(--fg)' },
  '.cm-panels.cm-panels-top': { borderBottom: '1px solid var(--border)' },
});

export interface Editor {
  view: EditorView;
  /** A fresh state (own undo history) for a tab that is not shown yet. */
  createState(text: string, opts?: { readOnly?: boolean }): EditorState;
  /** Shows a tab's state, keeping its undo history and selection, and restores its scroll position. */
  show(state: EditorState, scrollTop?: number): void;
  text(): string;
  undo(): void;
  redo(): void;
  openFind(): void;
  /** Opens the search panel with the cursor in its replace field. */
  openReplace(): void;
  /** Puts the cursor at the start of a 0-based source line and scrolls it to the top. */
  goToLine(line: number): void;
  setWordWrap(on: boolean): void;
  /** Cursor offset and scroll position, to restore after reloading the same file. */
  viewPosition(): ViewPosition;
  restoreViewPosition(position: ViewPosition): void;
}

interface ViewPosition {
  cursor: number;
  scrollTop: number;
}

export function createEditor(parent: HTMLElement, onUpdate: (u: ViewUpdate) => void): Editor {
  // Each tab owns a state, so word wrap lives in a compartment that show() re-applies.
  const wrap = new Compartment();
  let wordWrap = true;
  const wrapExtension = () => (wordWrap ? EditorView.lineWrapping : []);

  const extensions = (readOnly: boolean): Extension[] => [
    highlightSpecialChars(),
    history(),
    drawSelection(),
    dropCursor(),
    rectangularSelection(),
    indentOnInput(),
    bracketMatching(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    search({ top: true }),
    // YAML frontmatter (SKILL.md, docs sites, AI config files) is highlighted as YAML.
    // No tag completion: the editor has no autocompletion UI, and markdown-html.ts stubs it out.
    yamlFrontmatter({ content: markdown({ base: markdownLanguage, codeLanguages: languages, completeHTMLTags: false, extensions: markdownInlineSyntax }) }),
    syntaxHighlighting(highlight),
    wrap.of(wrapExtension()),
    EditorView.contentAttributes.of({ spellcheck: 'false', 'aria-label': 'Markdown source' }),
    theme,
    EditorState.readOnly.of(readOnly),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    EditorView.updateListener.of(onUpdate),
  ];

  const view = new EditorView({ parent, state: EditorState.create({ extensions: extensions(false) }) });
  // Menu commands arrive while focus is on the menu bar; return it to the editor.
  const runAndFocus = (command: (target: EditorView) => boolean) => {
    view.focus();
    command(view);
  };

  const createState = (text: string, opts: { readOnly?: boolean } = {}) =>
    EditorState.create({ doc: text, extensions: extensions(!!opts.readOnly) });
  const show = (state: EditorState, scrollTop = 0) => {
    view.setState(state);
    // A background tab may predate the last word-wrap change.
    if (wrap.get(state) !== wrapExtension()) view.dispatch({ effects: wrap.reconfigure(wrapExtension()) });
    // Focusing here forces the layout synchronously. Keep it: deferring it to the next frame
    // lets the preview's HTML land before the first paint and delays it by ~100 ms (measured).
    view.focus();
    // Wait for the new document's layout before scrolling.
    if (scrollTop) requestAnimationFrame(() => (view.scrollDOM.scrollTop = scrollTop));
  };

  return {
    view,
    createState,
    show,
    text: () => view.state.doc.toString(),
    undo: () => runAndFocus(undo),
    redo: () => runAndFocus(redo),
    openFind: () => runAndFocus(openSearchPanel),
    openReplace() {
      openSearchPanel(view);
      view.dom.querySelector<HTMLInputElement>('.cm-search input[name=replace]')?.focus();
    },
    goToLine(line) {
      const { doc } = view.state;
      const pos = doc.line(Math.min(Math.max(line + 1, 1), doc.lines)).from;
      view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: 'start' }) });
      view.focus();
    },
    setWordWrap(on) {
      if (on === wordWrap) return;
      wordWrap = on;
      view.dispatch({ effects: wrap.reconfigure(wrapExtension()) });
    },
    viewPosition: () => ({ cursor: view.state.selection.main.head, scrollTop: view.scrollDOM.scrollTop }),
    restoreViewPosition({ cursor, scrollTop }) {
      view.dispatch({ selection: { anchor: Math.min(cursor, view.state.doc.length) } });
      // Wait for the new document's layout before scrolling.
      requestAnimationFrame(() => (view.scrollDOM.scrollTop = scrollTop));
    },
  };
}
