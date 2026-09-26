// Shown only when the frontend runs in a plain browser (no Rust core).
export const SAMPLE = `# Marky

A fast Markdown editor for Windows. **Bold**, *italic*, ~~strike~~, \`inline code\` and https://example.com autolinks.

## Tasks

- [x] Scaffold Tauri 2 + CodeMirror 6
- [ ] Measure idle RAM and cold start
- [ ] Atomic save kill test

## Table

| Metric | Target | Hard limit |
| --- | --- | --- |
| Cold start | 400 ms | 700 ms |
| Idle RAM | 90 MB | 120 MB |

## Code

\`\`\`rust
fn main() {
    println!("hello from {}", "marky");
}
\`\`\`

\`\`\`ts
const answer: number = 42;
\`\`\`

> Raw HTML such as <script>alert(1)</script> is shown as text.
`;
