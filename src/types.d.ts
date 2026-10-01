declare module 'markdown-it-task-lists' {
  import type { MarkdownIt } from 'markdown-it';
  const taskLists: (md: MarkdownIt, options?: { enabled?: boolean; label?: boolean; labelAfter?: boolean }) => void;
  export default taskLists;
}

declare module 'markdown-it-sub' {
  import type { MarkdownIt } from 'markdown-it';
  const subscript: (md: MarkdownIt) => void;
  export default subscript;
}

declare module 'markdown-it-sup' {
  import type { MarkdownIt } from 'markdown-it';
  const superscript: (md: MarkdownIt) => void;
  export default superscript;
}

declare module 'markdown-it-ins' {
  import type { MarkdownIt } from 'markdown-it';
  const inserted: (md: MarkdownIt) => void;
  export default inserted;
}

declare module 'markdown-it-mark' {
  import type { MarkdownIt } from 'markdown-it';
  const marked: (md: MarkdownIt) => void;
  export default marked;
}

declare module 'markdown-it-deflist' {
  import type { MarkdownIt } from 'markdown-it';
  const definitionLists: (md: MarkdownIt) => void;
  export default definitionLists;
}

declare module 'markdown-it-abbr' {
  import type { MarkdownIt } from 'markdown-it';
  const abbreviations: (md: MarkdownIt) => void;
  export default abbreviations;
}

declare module 'markdown-it-footnote' {
  import type { MarkdownIt } from 'markdown-it';
  const footnote: (md: MarkdownIt) => void;
  export default footnote;
}
