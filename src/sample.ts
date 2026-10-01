// Shown only when the frontend runs in a plain browser (no Rust core).
export const SAMPLE = `---
name: marky
description: A fast Markdown editor for Windows
tags: [markdown, docs, ai]
---

# Marky

Fast Markdown for docs, READMEs and AI context files.[^small]

H~2~O, 19^th^, ++inserted++ and ==marked== text, and HTML as an abbreviation.

Preview
:   Rendered in a worker, scrolled in sync with the editor.

> Raw HTML such as <script>alert(1)</script> is shown as text, never run.

## Tasks

- [x] Tabs, menu bar, drag and drop
- [ ] Mermaid diagrams

| Metric | Target | Hard limit |
| --- | --- | --- |
| Cold start | 800 ms | 1,000 ms |
| Idle RAM | 90 MB | 120 MB |

## Math

Inline $E = mc^2$ and $\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}$, while prices like $5 and $10 stay text.

$$
\\operatorname{softmax}(x_i) = \\frac{e^{x_i}}{\\sum_j e^{x_j}}
$$

\`\`\`math
\\mathrm{Attention}(Q, K, V) = \\mathrm{softmax}\\left(\\frac{QK^\\top}{\\sqrt{d_k}}\\right)V
\`\`\`

Broken TeX stays readable: $\\frac{1}{$.

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

[^small]: Small and fast is the point: see the budgets in PRD.md.

*[HTML]: Hyper Text Markup Language
`;
