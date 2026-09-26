import { EditorState, type Extension } from '@codemirror/state';
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
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { HighlightStyle, bracketMatching, indentOnInput, syntaxHighlighting } from '@codemirror/language';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from '@codemirror/search';
import { tags as t } from '@lezer/highlight';

// Colours come from CSS variables so the OS light/dark switch needs no JS.
const highlight = HighlightStyle.define([
  { tag: t.heading, fontWeight: '700', color: 'var(--hl-heading)' },
  { tag: t.strong, fontWeight: '700' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.strikethrough, textDecoration: 'line-through' },
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
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.6', padding: '16px 0' },
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
  /** Replace the document and reset undo history (used when opening a file). */
  load(text: string, opts?: { readOnly?: boolean }): void;
  text(): string;
  openReplace(): void;
}

export function createEditor(parent: HTMLElement, onUpdate: (u: ViewUpdate) => void): Editor {
  const extensions = (ro: boolean): Extension[] => [
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
    markdown({ base: markdownLanguage, codeLanguages: languages }),
    syntaxHighlighting(highlight),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({ spellcheck: 'false', 'aria-label': 'Markdown source' }),
    theme,
    EditorState.readOnly.of(ro),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    EditorView.updateListener.of(onUpdate),
  ];

  const view = new EditorView({ parent, state: EditorState.create({ extensions: extensions(false) }) });

  return {
    view,
    load(text, opts = {}) {
      view.setState(EditorState.create({ doc: text, extensions: extensions(!!opts.readOnly) }));
      view.focus();
    },
    text: () => view.state.doc.toString(),
    openReplace: () => openSearchPanel(view),
  };
}
