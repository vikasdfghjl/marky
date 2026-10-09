/** One top-level Markdown block, rendered on its own in the worker. */
export interface RenderedBlock {
  html: string;
  /**
   * Identifies the block's content ignoring where it sits: `data-line` values are hashed
   * relative to `line`. Equal signatures mean the same DOM, up to a uniform line shift.
   */
  signature: string;
  /** 0-based source line the block starts on. */
  line: number;
}

interface LiveBlock {
  signature: string;
  line: number;
  nodes: ChildNode[];
}

/** Never appears in rendered output: markdown-it escapes all text and `html: false` drops raw HTML. */
const BLOCK_END_MARK = '|';
const BLOCK_END = `<!--${BLOCK_END_MARK}-->`;

/**
 * Keeps a container showing a list of blocks by replacing only what changed. Typing edits one
 * block, so the browser rebuilds one block instead of the whole document; the rest of the DOM
 * (drawn Mermaid diagrams, scroll position, layout) stays as it is. Replacing everything on
 * every render took 0.9 s per render at 1 MB and held hundreds of MB of dead DOM.
 */
export function createBlockDom(container: HTMLElement) {
  let live: LiveBlock[] = [];
  let replaceEverything = false;
  // A detached div parses like `container.innerHTML` did (scripts never run). A <template> would
  // work too, but its content lives in a second inert Document that costs about 2 MB of RAM.
  const scratch = document.createElement('div');

  return {
    apply(blocks: RenderedBlock[]) {
      const oldCount = live.length;
      const newCount = blocks.length;
      const prefix = replaceEverything ? 0 : commonPrefix(live, blocks);
      const suffix = replaceEverything ? 0 : commonSuffix(live, blocks, prefix);
      replaceEverything = false;

      const nextNode = firstNodeFrom(live, oldCount - suffix);
      for (let i = prefix; i < oldCount - suffix; i++) live[i].nodes.forEach((node) => node.remove());
      const inserted = buildBlocks(scratch, blocks.slice(prefix, newCount - suffix));
      container.insertBefore(inserted.fragment, nextNode);

      // Kept blocks may have moved (a line was added or removed above them).
      for (let i = 0; i < prefix; i++) followLine(live[i], blocks[i].line);
      for (let k = 1; k <= suffix; k++) followLine(live[oldCount - k], blocks[newCount - k].line);
      live = [...live.slice(0, prefix), ...inserted.blocks, ...live.slice(oldCount - suffix)];
    },

    /** The next `apply` replaces every block (Mermaid SVGs are theme-specific, for instance). */
    replaceEverythingNextTime() {
      replaceEverything = true;
    },
  };
}

function commonPrefix(live: LiveBlock[], blocks: RenderedBlock[]): number {
  const limit = Math.min(live.length, blocks.length);
  let count = 0;
  while (count < limit && live[count].signature === blocks[count].signature) count++;
  return count;
}

function commonSuffix(live: LiveBlock[], blocks: RenderedBlock[], prefix: number): number {
  const limit = Math.min(live.length, blocks.length) - prefix;
  let count = 0;
  while (count < limit && live[live.length - 1 - count].signature === blocks[blocks.length - 1 - count].signature) count++;
  return count;
}

function firstNodeFrom(live: LiveBlock[], from: number): ChildNode | null {
  for (let i = from; i < live.length; i++) {
    if (live[i].nodes.length) return live[i].nodes[0];
  }
  return null;
}

/** Parses all the blocks in one go, then splits the nodes back into blocks at the end markers. */
function buildBlocks(scratch: HTMLElement, blocks: RenderedBlock[]) {
  const fragment = document.createDocumentFragment();
  const built: LiveBlock[] = [];
  if (blocks.length === 0) return { fragment, blocks: built };

  scratch.innerHTML = blocks.map((block) => block.html + BLOCK_END).join('');
  let nodes: ChildNode[] = [];
  for (const node of [...scratch.childNodes]) {
    if (node.nodeType === Node.COMMENT_NODE && (node as Comment).data === BLOCK_END_MARK) {
      const block = blocks[built.length];
      built.push({ signature: block.signature, line: block.line, nodes });
      nodes = [];
    } else {
      nodes.push(node);
      fragment.append(node);
    }
  }
  scratch.replaceChildren(); // drop the end markers
  return { fragment, blocks: built };
}

/** Moves a kept block's `data-line` values by however far the block moved. */
function followLine(live: LiveBlock, line: number) {
  const delta = line - live.line;
  if (delta === 0) return;
  live.line = line;
  for (const node of live.nodes) {
    if (!(node instanceof Element)) continue;
    const marked = node.hasAttribute('data-line') ? [node] : [];
    for (const el of [...marked, ...node.querySelectorAll('[data-line]')]) {
      el.setAttribute('data-line', String(Number(el.getAttribute('data-line')) + delta));
    }
  }
}
