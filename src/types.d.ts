declare module 'markdown-it-task-lists' {
  import type { MarkdownIt } from 'markdown-it';
  const taskLists: (md: MarkdownIt, options?: { enabled?: boolean; label?: boolean; labelAfter?: boolean }) => void;
  export default taskLists;
}

// @types/markdown-it-emoji depends on @types/markdown-it, which conflicts with markdown-it 15's own types.
declare module 'markdown-it-emoji' {
  import type { MarkdownIt } from 'markdown-it';
  type EmojiPlugin = (md: MarkdownIt, options?: { defs?: Record<string, string>; enabled?: string[]; shortcuts?: Record<string, string | string[]> }) => void;
  export const full: EmojiPlugin;
  export const light: EmojiPlugin;
  export const bare: EmojiPlugin;
}
