//! Headings, paragraphs, quotes, lists, emphasis, code, display math, and Mermaid fences.
//! Math is the TeX source: there is no DOM for KaTeX.
//! A diagram is the Mermaid source: this window does not run Mermaid.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    Quote,
    /// `ordered` is the item number. `None` is a bullet.
    Item { ordered: Option<u64> },
    Code,
    Math,
    Diagram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    /// Plain text of `runs`, for the outline and tests.
    pub text: String,
    pub runs: Vec<Run>,
    /// 1-based line of the block's start in the LF document `files::read` returns.
    pub line: usize,
}

#[derive(Clone, Copy)]
enum Open {
    Heading(u8),
    Paragraph,
    Item { ordered: Option<u64> },
    Code,
    Math,
    Diagram,
}

struct Parse {
    open: Option<Open>,
    quote: usize,
    lists: Vec<Option<u64>>,
    style: RunStyle,
    runs: Vec<Run>,
    line: usize,
}

pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut parse = Parse {
        open: None,
        quote: 0,
        lists: Vec::new(),
        style: RunStyle::default(),
        runs: Vec::new(),
        line: 1,
    };
    // Frontmatter is metadata, as in the app's preview; without the option it parses as a rule and a heading.
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;

    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => parse.begin(Open::Heading(level as u8), markdown, range.start, &mut out),
            Event::Start(Tag::Paragraph) if !matches!(parse.open, Some(Open::Item { .. })) => {
                parse.begin(Open::Paragraph, markdown, range.start, &mut out);
            }
            Event::Start(Tag::BlockQuote(_)) => parse.quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => parse.quote = parse.quote.saturating_sub(1),
            Event::Start(Tag::List(start)) => parse.lists.push(start),
            Event::End(TagEnd::List(_)) => {
                parse.lists.pop();
            }
            Event::Start(Tag::Item) => {
                let ordered = parse.lists.last_mut().and_then(|next| {
                    let number = (*next)?;
                    *next = Some(number + 1);
                    Some(number)
                });
                parse.begin(Open::Item { ordered }, markdown, range.start, &mut out);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let open = match kind {
                    CodeBlockKind::Fenced(lang) if lang.as_ref() == "math" => Open::Math,
                    CodeBlockKind::Fenced(lang) if lang.as_ref() == "mermaid" => Open::Diagram,
                    _ => Open::Code,
                };
                parse.begin(open, markdown, range.start, &mut out);
            }
            Event::Start(Tag::Emphasis) => parse.style.italic = true,
            Event::End(TagEnd::Emphasis) => parse.style.italic = false,
            Event::Start(Tag::Strong) => parse.style.bold = true,
            Event::End(TagEnd::Strong) => parse.style.bold = false,
            Event::Start(Tag::Strikethrough) => parse.style.strike = true,
            Event::End(TagEnd::Strikethrough) => parse.style.strike = false,
            Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => parse.begin(Open::Paragraph, markdown, range.start, &mut out),
            Event::Start(Tag::TableCell) if parse.open.is_some() && !parse.runs.is_empty() => parse.push(" | ", false),
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::CodeBlock) | Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                parse.finish(&mut out);
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(math) = display_math(&parse.plain()) {
                    parse.runs = vec![Run { text: math, style: RunStyle::default() }];
                    parse.open = Some(Open::Math);
                }
                if !matches!(parse.open, Some(Open::Item { .. })) {
                    parse.finish(&mut out);
                }
            }
            Event::End(TagEnd::Item) => parse.finish(&mut out),
            Event::Text(fragment) if parse.open.is_some() => parse.push(&fragment, false),
            Event::Code(fragment) if parse.open.is_some() => parse.push(&fragment, true),
            Event::SoftBreak | Event::HardBreak if matches!(parse.open, Some(Open::Code | Open::Math | Open::Diagram)) => parse.push("\n", false),
            Event::SoftBreak | Event::HardBreak if parse.open.is_some() => parse.push(" ", false),
            _ => {}
        }
    }
    out
}

impl Parse {
    /// A block that starts while another is open (a nested list item, a code block in an item)
    /// ends the outer block first, so the outer text is kept instead of cleared.
    fn begin(&mut self, open: Open, markdown: &str, byte: usize, out: &mut Vec<Block>) {
        self.finish(out);
        self.open = Some(open);
        self.runs.clear();
        self.line = line_at(markdown, byte);
    }

    fn push(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let mut style = self.style;
        style.code = code;
        if let Some(last) = self.runs.last_mut() {
            if last.style == style {
                last.text.push_str(text);
                return;
            }
        }
        self.runs.push(Run { text: text.to_string(), style });
    }

    fn plain(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    fn finish(&mut self, out: &mut Vec<Block>) {
        let Some(open) = self.open.take() else { return };
        let mut runs = std::mem::take(&mut self.runs);
        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if runs.len() == 1 {
            runs[0].text = text.clone();
        }
        let kind = match open {
            Open::Heading(level) => Kind::Heading(level),
            Open::Paragraph if self.quote > 0 => Kind::Quote,
            Open::Paragraph => Kind::Paragraph,
            Open::Item { ordered } => Kind::Item { ordered },
            Open::Code => Kind::Code,
            Open::Math => Kind::Math,
            Open::Diagram => Kind::Diagram,
        };
        out.push(Block { kind, text, runs, line: self.line });
    }
}

/// A paragraph that is only a `$$...$$` block. Inline `$...$` stays inside the paragraph.
fn display_math(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("$$")?.strip_suffix("$$")?.trim();
    if inner.is_empty() { None } else { Some(inner.to_string()) }
}

fn line_at(markdown: &str, byte: usize) -> usize {
    markdown[..byte.min(markdown.len())].bytes().filter(|b| *b == b'\n').count() + 1
}

/// The last block that starts on or before `line` (1-based). Used to keep the preview beside the source.
pub fn block_at_line(blocks: &[Block], line: usize) -> usize {
    let mut index = 0;
    for (i, block) in blocks.iter().enumerate() {
        if block.line > line {
            break;
        }
        index = i;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(kind: Kind, text: &str, line: usize) -> Block {
        Block {
            kind,
            text: text.into(),
            runs: vec![Run { text: text.into(), style: RunStyle::default() }],
            line,
        }
    }

    #[test]
    fn headings_paragraphs_and_list_items_keep_their_source_line() {
        let markdown = "# Title\n\nHello there.\n\n## Next\n\n- skipped\n\nAfter the list.\n";
        let blocks = blocks(markdown);
        assert_eq!(
            blocks,
            vec![
                plain(Kind::Heading(1), "Title", 1),
                plain(Kind::Paragraph, "Hello there.", 3),
                plain(Kind::Heading(2), "Next", 5),
                plain(Kind::Item { ordered: None }, "skipped", 7),
                plain(Kind::Paragraph, "After the list.", 9),
            ]
        );
    }

    #[test]
    fn emphasis_is_a_styled_run_and_strikethrough_drops_the_markers() {
        let blocks = blocks("**bold** and *italic* and ~~gone~~\n");
        assert_eq!(blocks[0].text, "bold and italic and gone");
        assert!(blocks[0].runs[0].style.bold);
        assert!(blocks[0].runs[2].style.italic);
        assert!(blocks[0].runs[4].style.strike);
    }

    #[test]
    fn math_and_mermaid_fences_keep_their_source() {
        let markdown = "# T\n\n```math\nE=mc^2\n```\n\n```mermaid\nflowchart LR\n  A-->B\n```\n\n$$ x+1 $$\n";
        let blocks = blocks(markdown);
        assert_eq!(blocks[1].kind, Kind::Math);
        assert_eq!(blocks[1].text, "E=mc^2");
        assert_eq!(blocks[1].line, 3);
        assert_eq!(blocks[2].kind, Kind::Diagram);
        assert!(blocks[2].text.contains("A-->B"));
        assert_eq!(blocks[3].kind, Kind::Math);
        assert_eq!(blocks[3].text, "x+1");
        assert_eq!(blocks[3].line, 12);
    }

    #[test]
    fn nested_list_items_keep_the_parent_text() {
        let blocks = blocks("- parent\n  - child\n- next\n");
        let texts: Vec<&str> = blocks.iter().map(|block| block.text.as_str()).collect();
        assert_eq!(texts, ["parent", "child", "next"]);
        assert_eq!(blocks[1].line, 2);
    }

    #[test]
    fn frontmatter_is_not_a_heading() {
        let blocks = blocks("---\nname: deploy\n---\n\n# Title\n");
        assert_eq!(blocks, vec![plain(Kind::Heading(1), "Title", 5)]);
    }

    #[test]
    fn block_at_line_uses_the_latest_block_at_or_above_the_line() {
        let blocks = blocks("# Title\n\nHello there.\n\n## Next\n");
        assert_eq!(block_at_line(&blocks, 1), 0);
        assert_eq!(block_at_line(&blocks, 4), 1);
        assert_eq!(block_at_line(&blocks, 5), 2);
    }
}
