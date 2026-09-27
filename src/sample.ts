// Shown only when the frontend runs in a plain browser (no Rust core).
export const SAMPLE = `---
name: marky
description: A fast Markdown editor for Windows
tags: [markdown, docs, ai]
---

# Marky

Fast Markdown for docs, READMEs and AI context files. Jump to [the code](#code-blocks).

> [!NOTE]
> GitHub alerts render as callouts: NOTE, TIP, IMPORTANT, WARNING, CAUTION.

> [!WARNING]
> Raw HTML such as <script>alert(1)</script> is shown as text, never run.

## Tasks

- [x] Tabs, menu bar, drag and drop
- [ ] Mermaid diagrams

| Metric | Target | Hard limit |
| --- | --- | --- |
| Cold start | 800 ms | 1,000 ms |
| Idle RAM | 90 MB | 120 MB |

## Architecture

\`\`\`mermaid
flowchart LR
  Rust[Rust core] -- file text --> CM[CodeMirror]
  CM -- debounced doc --> Worker[markdown-it worker]
  Worker -- HTML --> Preview
\`\`\`

## Code blocks

\`\`\`rust
fn main() {
    let name = "marky"; // highlighted with the editor's parsers
    println!("hello from {name}");
}
\`\`\`

\`\`\`ts
export const answer: number = 42;
\`\`\`

\`\`\`json
{ "model": "claude", "maxTokens": 1024 }
\`\`\`
`;
