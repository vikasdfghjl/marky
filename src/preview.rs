//! The preview's blocks from a pulldown-cmark parse: headings, paragraphs, quotes, lists and task
//! lists, code, tables, footnotes, definition lists, images, display math and Mermaid fences, with
//! inline emphasis, strikethrough, code, links, `~sub~`, `^sup^` and `==mark==`.
//! Math is TeX for RaTeX to lay out (math.rs): inline and display formulas, and math fences.
//! Diagrams are Mermaid fences rendered by diagrams.rs. Abbreviations, bare links,
//! the typographer and the app's math rule for `$…$` come from extras.rs.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::extras::{self, Abbreviations};
use crate::syntax::{self, Span};

/// The syntax the app renders that pulldown-cmark knows: CommonMark, GFM tables, task lists and
/// strikethrough, footnotes, definition lists, `~sub~` and `^sup^` (one tilde is subscript, two are
/// strikethrough, as in the app), and frontmatter, which is metadata rather than a rule and a heading.
pub fn options() -> Options {
    Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    Quote,
    /// `ordered` is the item number (`None` is a bullet); `checked` marks a task list item.
    Item { ordered: Option<u64>, checked: Option<bool> },
    Code,
    Math,
    Diagram,
    /// The cells are in `Block::table`.
    Table,
    /// A footnote's text; the number is the one its references show.
    Footnote(u32),
    /// A definition list's term, and one of its definitions.
    Term,
    Definition,
    /// A thematic break (`---`), and the rule markdown-it puts above the footnotes.
    Rule,
    /// A paragraph that is only an image. The path or URL is in `Block::src`, the alt text in `text`.
    Image,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A GFM table. The first row is the header; a row can have fewer cells than there are columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub align: Vec<Align>,
    pub rows: Vec<Vec<Vec<Run>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub sub: bool,
    pub sup: bool,
    pub mark: bool,
    /// The run is TeX: inline `$…$`, or `$$…$$` (display) inside a paragraph.
    pub math: bool,
    pub display: bool,
    /// An abbreviation's title (`*[HTML]: Hyper Text Markup Language`).
    pub abbr: Option<Arc<str>>,
    /// The link's destination, as written: a URL, a relative path, or `#fn:N` for a footnote reference.
    pub link: Option<Arc<str>>,
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
    pub table: Option<Table>,
    /// tree-sitter colours for a code block in a known language, relative to `text`.
    pub code: Option<Arc<Vec<Span>>>,
    /// An image block's path or URL, as written.
    pub src: Option<String>,
}

#[derive(Clone, Copy)]
enum Open {
    Heading(u8),
    Paragraph,
    Item { ordered: Option<u64>, checked: Option<bool> },
    Code,
    Math,
    Diagram,
    Table,
    Term,
    Definition,
}

impl Open {
    /// Containers whose paragraphs are part of the block itself (a tight list item, a definition).
    fn holds_paragraphs(self) -> bool {
        matches!(self, Open::Item { .. } | Open::Definition)
    }
}

struct Parse {
    open: Option<Open>,
    quote: usize,
    lists: Vec<Option<u64>>,
    style: RunStyle,
    runs: Vec<Run>,
    line: usize,
    lines: LineCounter,
    table: Option<Table>,
    /// The open code block's info string (```rust → "rust").
    code_info: String,
    /// Footnote numbers by label, in order of first use, reference or definition.
    footnotes: HashMap<String, u32>,
    /// The footnote definition being read; its paragraphs are footnote blocks.
    footnote: Option<u32>,
    /// The image being read: its destination, and where its alt text starts in `runs`.
    image: Option<(String, usize)>,
    /// Footnote numbers that something refers to; markdown-it shows only those footnotes.
    referenced: HashSet<u32>,
    abbreviations: Abbreviations,
    typographer: bool,
    /// The open paragraph is only abbreviation definitions, which the app does not show.
    hidden: bool,
}

/// What the preview's parse depends on besides the text: the app's `typographer` setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flavor {
    pub typographer: bool,
}

/// Line numbers for block starts. Blocks start in document order, so each lookup counts only the
/// newlines since the previous one; counting from the start every time made a 1 MB parse take seconds.
#[derive(Default)]
struct LineCounter {
    byte: usize,
    newlines: usize,
}

impl LineCounter {
    /// 1-based line of `byte`.
    fn line_at(&mut self, markdown: &str, byte: usize) -> usize {
        let byte = byte.min(markdown.len());
        if byte < self.byte {
            *self = Self::default();
        }
        self.newlines += markdown.as_bytes()[self.byte..byte].iter().filter(|b| **b == b'\n').count();
        self.byte = byte;
        self.newlines + 1
    }
}

/// The blocks without smart punctuation, for tests.
#[cfg(test)]
pub fn blocks(markdown: &str) -> Vec<Block> {
    blocks_with(markdown, Flavor::default())
}

pub fn blocks_with(markdown: &str, flavor: Flavor) -> Vec<Block> {
    let mut options = options();
    // Quotes, dashes and ellipses; extras::replacements adds (c), (r), (tm) and +-.
    if flavor.typographer {
        options |= Options::ENABLE_SMART_PUNCTUATION;
    }
    let abbreviations = if markdown.contains("*[") { extras::collect_abbreviations(markdown) } else { Abbreviations::default() };
    let mut out = Vec::new();
    let mut parse = Parse {
        open: None,
        quote: 0,
        lists: Vec::new(),
        style: RunStyle::default(),
        runs: Vec::new(),
        line: 1,
        lines: LineCounter::default(),
        table: None,
        code_info: String::new(),
        footnotes: HashMap::new(),
        footnote: None,
        image: None,
        referenced: HashSet::new(),
        abbreviations,
        typographer: flavor.typographer,
        hidden: false,
    };

    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => parse.begin(Open::Heading(level as u8), markdown, range.start, &mut out),
            Event::Start(Tag::Paragraph) if !parse.open.is_some_and(Open::holds_paragraphs) => {
                parse.begin(Open::Paragraph, markdown, range.start, &mut out);
                let source = &markdown[range.clone()];
                parse.hidden = !parse.abbreviations.is_empty() && extras::only_definitions(source);
            }
            Event::Rule => {
                parse.finish(&mut out);
                let line = parse.lines.line_at(markdown, range.start);
                out.push(Block { kind: Kind::Rule, text: String::new(), runs: Vec::new(), line, table: None, code: None, src: None });
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
                parse.begin(Open::Item { ordered, checked: None }, markdown, range.start, &mut out);
            }
            Event::TaskListMarker(done) => {
                if let Some(Open::Item { checked, .. }) = parse.open.as_mut() {
                    *checked = Some(done);
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match &kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let open = match info.as_str() {
                    "math" => Open::Math,
                    "mermaid" => Open::Diagram,
                    _ => Open::Code,
                };
                parse.begin(open, markdown, range.start, &mut out);
                parse.code_info = info;
            }
            Event::Start(Tag::Emphasis) => parse.style.italic = true,
            Event::End(TagEnd::Emphasis) => parse.style.italic = false,
            Event::Start(Tag::Strong) => parse.style.bold = true,
            Event::End(TagEnd::Strong) => parse.style.bold = false,
            Event::Start(Tag::Strikethrough) => parse.style.strike = true,
            Event::End(TagEnd::Strikethrough) => parse.style.strike = false,
            Event::Start(Tag::Subscript) => parse.style.sub = true,
            Event::End(TagEnd::Subscript) => parse.style.sub = false,
            Event::Start(Tag::Superscript) => parse.style.sup = true,
            Event::End(TagEnd::Superscript) => parse.style.sup = false,
            Event::Start(Tag::Link { dest_url, .. }) => parse.style.link = Some(Arc::from(dest_url.as_ref())),
            Event::End(TagEnd::Link) => parse.style.link = None,
            Event::Start(Tag::Image { dest_url, .. }) => parse.image = Some((dest_url.to_string(), parse.runs.len())),
            Event::End(TagEnd::Image) => parse.end_image(),
            Event::FootnoteReference(label) if parse.open.is_some() => {
                let number = parse.footnote_number(&label);
                parse.referenced.insert(number);
                let style = RunStyle { sup: true, link: Some(Arc::from(format!("#fn:{number}"))), ..RunStyle::default() };
                parse.runs.push(Run { text: format!("[{number}]"), style });
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                parse.finish(&mut out);
                parse.footnote = Some(parse.footnote_number(&label));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                parse.finish(&mut out);
                parse.footnote = None;
            }
            Event::Start(Tag::DefinitionListTitle) => parse.begin(Open::Term, markdown, range.start, &mut out),
            Event::Start(Tag::DefinitionListDefinition) => parse.begin(Open::Definition, markdown, range.start, &mut out),
            Event::End(TagEnd::DefinitionListTitle) | Event::End(TagEnd::DefinitionListDefinition) => parse.finish(&mut out),
            Event::Start(Tag::Table(align)) => {
                parse.begin(Open::Table, markdown, range.start, &mut out);
                parse.table = Some(Table { align: align.iter().map(align_of).collect(), rows: Vec::new() });
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                if let Some(table) = &mut parse.table {
                    table.rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => parse.runs.clear(),
            Event::End(TagEnd::TableCell) => {
                let runs = std::mem::take(&mut parse.runs);
                let cell = parse.inline(runs);
                if let Some(row) = parse.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::CodeBlock) | Event::End(TagEnd::Table) => {
                parse.finish(&mut out);
            }
            Event::End(TagEnd::Paragraph) => {
                // A paragraph that is only `$$…$$` is a display formula block, as in the app, also
                // when pulldown-cmark did not take it as math (the app's rule does not need
                // balanced braces, so broken TeX shows its error).
                if matches!(parse.open, Some(Open::Paragraph)) {
                    let tex = match parse.runs.as_slice() {
                        [run] if run.style.display => Some(run.text.clone()),
                        runs if runs.iter().all(|run| run.style == RunStyle::default()) => extras::display_math(&plain_text(runs)).map(str::to_string),
                        _ => None,
                    };
                    if let Some(tex) = tex {
                        parse.runs = vec![Run { text: tex, style: RunStyle::default() }];
                        parse.open = Some(Open::Math);
                    }
                }
                if !parse.open.is_some_and(Open::holds_paragraphs) {
                    parse.finish(&mut out);
                }
            }
            Event::End(TagEnd::Item) => parse.finish(&mut out),
            Event::Text(fragment) if parse.open.is_some() => parse.push_text(&fragment),
            Event::Code(fragment) if parse.open.is_some() => parse.push(&fragment, true),
            Event::InlineMath(tex) if parse.open.is_some() => parse.push_math(&tex, false),
            Event::DisplayMath(tex) if parse.open.is_some() => parse.push_math(&tex, true),
            Event::SoftBreak | Event::HardBreak if matches!(parse.open, Some(Open::Code | Open::Math | Open::Diagram)) => parse.push("\n", false),
            Event::SoftBreak | Event::HardBreak if parse.open.is_some() => parse.push(" ", false),
            _ => {}
        }
    }
    parse.finish(&mut out);
    gather_footnotes(out, &parse.referenced)
}

/// markdown-it-footnote shows the footnotes at the end of the document, under a rule, in the order
/// of their numbers, each ending with a link back to its reference; footnotes nothing refers to are
/// left out. Their blocks keep their own source lines, so the editor can still find them.
fn gather_footnotes(blocks: Vec<Block>, referenced: &HashSet<u32>) -> Vec<Block> {
    if !blocks.iter().any(|block| matches!(block.kind, Kind::Footnote(_))) {
        return blocks;
    }
    let (mut notes, mut body): (Vec<Block>, Vec<Block>) = blocks.into_iter().partition(|block| matches!(block.kind, Kind::Footnote(_)));
    notes.retain(|block| matches!(block.kind, Kind::Footnote(number) if referenced.contains(&number)));
    if notes.is_empty() {
        return body;
    }
    notes.sort_by_key(|block| match block.kind {
        Kind::Footnote(number) => number,
        _ => 0,
    });
    for index in 0..notes.len() {
        let Kind::Footnote(number) = notes[index].kind else { continue };
        let last_of_note = notes.get(index + 1).is_none_or(|next| next.kind != notes[index].kind);
        if last_of_note {
            let back = RunStyle { link: Some(Arc::from(format!("#fnref:{number}"))), ..RunStyle::default() };
            notes[index].runs.push(Run { text: " ↩".to_string(), style: back });
        }
    }
    let line = notes[0].line;
    body.push(Block { kind: Kind::Rule, text: String::new(), runs: Vec::new(), line, table: None, code: None, src: None });
    body.extend(notes);
    body
}

impl Parse {
    /// A block that starts while another is open (a nested list item, a code block in an item)
    /// ends the outer block first, so the outer text is kept instead of cleared.
    fn begin(&mut self, open: Open, markdown: &str, byte: usize, out: &mut Vec<Block>) {
        self.finish(out);
        self.open = Some(open);
        self.runs.clear();
        self.line = self.lines.line_at(markdown, byte);
    }

    /// Everything that happens to a block's text after pulldown-cmark: the app's `$…$` rule, then
    /// `~sub~`, `^sup^` and `==mark==`, then bare links, the typographer and abbreviations.
    fn inline(&self, runs: Vec<Run>) -> Vec<Run> {
        let runs = inline_extensions(extras::apply_math(runs));
        extras::apply_text(runs, &self.abbreviations, self.typographer)
    }

    fn footnote_number(&mut self, label: &str) -> u32 {
        let next = self.footnotes.len() as u32 + 1;
        *self.footnotes.entry(label.to_string()).or_insert(next)
    }

    /// A formula is its own run, never merged with text: the preview draws it as an image.
    fn push_math(&mut self, tex: &str, display: bool) {
        let style = RunStyle { math: true, display, ..self.style.clone() };
        self.runs.push(Run { text: tex.trim().to_string(), style });
    }

    fn push_text(&mut self, text: &str) {
        self.push(text, false);
    }

    fn push(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let mut style = self.style.clone();
        style.code = code;
        // An image's alt text starts a run of its own, so `end_image` finds all of it.
        let image_starts = self.image.as_ref().is_some_and(|(_, start)| *start == self.runs.len());
        if let Some(last) = self.runs.last_mut().filter(|_| !image_starts) {
            if last.style == style {
                last.text.push_str(text);
                return;
            }
        }
        self.runs.push(Run { text: text.to_string(), style });
    }

    /// An image's alt text was pushed as runs; they become one italic run carrying the destination,
    /// so a paragraph that is only an image can become an image block.
    fn end_image(&mut self) {
        let Some((src, start)) = self.image.take() else { return };
        let alt = plain_text(&self.runs[start.min(self.runs.len())..]);
        self.runs.truncate(start);
        let style = RunStyle { italic: true, link: Some(Arc::from(format!("image:{src}"))), ..RunStyle::default() };
        self.runs.push(Run { text: alt, style });
    }


    fn finish(&mut self, out: &mut Vec<Block>) {
        let Some(open) = self.open.take() else { return };
        if let Open::Table = open {
            self.finish_table(out);
            return;
        }
        let mut runs = std::mem::take(&mut self.runs);
        if std::mem::take(&mut self.hidden) {
            return;
        }
        if !matches!(open, Open::Code | Open::Math | Open::Diagram) {
            runs = self.inline(runs);
        }
        // A paragraph that is only an image is drawn as the image.
        if let (Open::Paragraph, [run]) = (open, runs.as_slice()) {
            if let Some(src) = run.style.link.as_deref().and_then(|link| link.strip_prefix("image:")) {
                let src = Some(src.to_string());
                out.push(Block { kind: Kind::Image, text: run.text.clone(), runs: Vec::new(), line: self.line, table: None, code: None, src });
                return;
            }
        }
        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        // Code keeps its first line's indentation; only the newline before the closing fence goes.
        let text = match open {
            Open::Code | Open::Diagram => text.trim_end_matches('\n').to_string(),
            _ => text.trim().to_string(),
        };
        if text.trim().is_empty() && !matches!(open, Open::Item { checked: Some(_), .. }) {
            return;
        }
        if runs.len() == 1 {
            runs[0].text = text.clone();
        }
        let kind = match open {
            Open::Heading(level) => Kind::Heading(level),
            Open::Paragraph if self.footnote.is_some() => Kind::Footnote(self.footnote.unwrap_or(0)),
            Open::Paragraph if self.quote > 0 => Kind::Quote,
            Open::Paragraph => Kind::Paragraph,
            Open::Item { ordered, checked } => Kind::Item { ordered, checked },
            Open::Code => Kind::Code,
            Open::Math => Kind::Math,
            Open::Diagram => Kind::Diagram,
            Open::Term => Kind::Term,
            Open::Definition => Kind::Definition,
            Open::Table => unreachable!("tables finish in finish_table"),
        };
        // The same tree-sitter result (and cache entry) the editor uses for this fence's code.
        let code = match kind {
            Kind::Code => syntax::highlight_code(&std::mem::take(&mut self.code_info), &text),
            _ => None,
        };
        out.push(Block { kind, text, runs, line: self.line, table: None, code, src: None });
    }

    /// `text` is the cells joined by ` | ` per row, for tests; the outline lists headings only.
    fn finish_table(&mut self, out: &mut Vec<Block>) {
        self.runs.clear();
        let Some(table) = self.table.take() else { return };
        if table.rows.is_empty() {
            return;
        }
        let text = table
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| plain_text(cell)).collect::<Vec<_>>().join(" | "))
            .collect::<Vec<_>>()
            .join("\n");
        out.push(Block { kind: Kind::Table, text, runs: Vec::new(), line: self.line, table: Some(table), code: None, src: None });
    }
}

/// `~sub~`, `^sup^` and `==mark==` in a block's plain text runs, with the app's markdown-it rules.
/// pulldown-cmark parses a single `~` or `^` like `_`, never inside a word, so `H~2~O` and `19^th^`
/// (the app's own examples) stay text there; this finds them. Code runs are left alone.
fn inline_extensions(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.code || run.style.math {
            out.push(run);
            continue;
        }
        let mut rest = run.text.as_str();
        while let Some((open, close, kind)) = next_extension(rest) {
            push_run(&mut out, &rest[..open], run.style.clone());
            let mut style = run.style.clone();
            let marker = match kind {
                Extension::Sub => {
                    style.sub = true;
                    1
                }
                Extension::Sup => {
                    style.sup = true;
                    1
                }
                Extension::Mark => {
                    style.mark = true;
                    2
                }
            };
            push_run(&mut out, &rest[open + marker..close], style);
            rest = &rest[close + marker..];
        }
        push_run(&mut out, rest, run.style);
    }
    out
}

fn push_run(out: &mut Vec<Run>, text: &str, style: RunStyle) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => out.push(Run { text: text.to_string(), style }),
    }
}

#[derive(Clone, Copy)]
enum Extension {
    Sub,
    Sup,
    Mark,
}

/// The earliest `~x~`, `^x^` or `==x==` in `text`: (opening offset, closing offset, kind).
fn next_extension(text: &str) -> Option<(usize, usize, Extension)> {
    [
        find_mark(text).map(|(open, close)| (open, close, Extension::Mark)),
        find_script(text, '~').map(|(open, close)| (open, close, Extension::Sub)),
        find_script(text, '^').map(|(open, close)| (open, close, Extension::Sup)),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(open, _, _)| *open)
}

/// The `~sub~` and `^sup^` content ranges in one line, as byte ranges without their delimiters, for
/// the editor's own smaller script text. `true` means superscript. `~~strikethrough~~` is not one.
pub fn script_ranges(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut ranges = Vec::new();
    let mut rest = text;
    let mut offset = 0;
    while let Some((open, close, kind)) = next_extension(rest) {
        if let Extension::Sub | Extension::Sup = kind {
            ranges.push((offset + open + 1..offset + close, matches!(kind, Extension::Sup)));
        }
        let next = close + 1;
        rest = &rest[next..];
        offset += next;
    }
    ranges
}

/// markdown-it-sub and -sup: a single `delim`, then text with no spaces, then `delim` again.
pub fn find_script(text: &str, delim: char) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(found) = text[from..].find(delim) {
        let open = from + found;
        let after = &text[open + 1..];
        let doubled = after.starts_with(delim) || text[..open].ends_with(delim);
        if let Some(length) = after.find(delim) {
            let inner = &after[..length];
            if !doubled && !inner.is_empty() && !inner.contains(char::is_whitespace) {
                return Some((open, open + 1 + length));
            }
        }
        from = open + 1;
    }
    None
}

/// Byte offsets of the next `==` pair that marks text, if any.
pub fn find_mark(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(found) = text[from..].find("==") {
        let open = from + found;
        let inner = &text[open + 2..];
        if inner.starts_with(|c: char| c.is_whitespace() || c == '=') || inner.is_empty() {
            from = open + 2;
            continue;
        }
        let mut search = 0;
        while let Some(found) = inner[search..].find("==") {
            let close = search + found;
            if close > 0 && !inner[..close].ends_with(char::is_whitespace) {
                return Some((open, open + 2 + close));
            }
            search = close + 2;
        }
        return None;
    }
    None
}

pub fn plain_text(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

fn align_of(align: &Alignment) -> Align {
    match align {
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
        Alignment::None | Alignment::Left => Align::Left,
    }
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
            table: None,
            code: None,
            src: None,
        }
    }

    fn bullet() -> Kind {
        Kind::Item { ordered: None, checked: None }
    }

    #[test]
    fn a_fenced_code_block_keeps_its_indentation_and_gets_colours() {
        let blocks = blocks("```python\n    x = 1  # one\n```\n\n```\nplain\n```\n");
        assert_eq!(blocks[0].text, "    x = 1  # one");
        assert!(blocks[0].code.as_ref().is_some_and(|spans| !spans.is_empty()));
        assert!(blocks[1].code.is_none());
    }

    #[test]
    fn a_table_is_one_block_with_a_header_row_cells_and_alignment() {
        let markdown = "Intro.\n\n| Env | Replicas |\n| :-- | --: |\n| staging | **2** |\n| canary |\n\nAfter.\n";
        let blocks = blocks(markdown);
        assert_eq!(blocks.len(), 3);
        let table = &blocks[1];
        assert_eq!(table.kind, Kind::Table);
        assert_eq!(table.line, 3);
        // GFM pads a short row with empty cells up to the header's column count.
        assert_eq!(table.text, "Env | Replicas\nstaging | 2\ncanary | ");
        let cells = table.table.as_ref().unwrap();
        assert_eq!(cells.align, [Align::Left, Align::Right]);
        assert_eq!(cells.rows.len(), 3);
        assert!(cells.rows.iter().all(|row| row.len() == 2));
        assert!(cells.rows[1][1][0].style.bold);
        assert_eq!(blocks[2].text, "After.");
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
                plain(bullet(), "skipped", 7),
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
    fn editor_script_ranges_exclude_delimiters_and_strikethrough() {
        assert_eq!(script_ranges("H~2~O"), vec![(2..3, false)]);
        assert_eq!(script_ranges("19^th^"), vec![(3..5, true)]);
        assert!(script_ranges("~~struck~~").is_empty());
        assert_eq!(script_ranges("a ~x~ b ^y^"), vec![(3..4, false), (9..10, true)]);
    }

    #[test]
    fn subscript_superscript_and_marked_text_are_styled_runs() {
        let blocks = blocks("H~2~O, 19^th^ and ==look here== but a == b, 2^ 3 and ~~gone~~.\n");
        let styled = |text: &str| blocks[0].runs.iter().find(|run| run.text == text).map(|run| run.style.clone()).unwrap();
        assert!(styled("2").sub);
        assert!(styled("th").sup);
        assert!(styled("look here").mark);
        assert!(styled("gone").strike);
        assert_eq!(blocks[0].text, "H2O, 19th and look here but a == b, 2^ 3 and gone.");
    }

    #[test]
    fn task_list_items_carry_their_state() {
        let blocks = blocks("- [ ] write\n- [x] test\n- plain\n");
        let kinds: Vec<Kind> = blocks.iter().map(|block| block.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Item { ordered: None, checked: Some(false) },
                Kind::Item { ordered: None, checked: Some(true) },
                bullet(),
            ]
        );
        assert_eq!(blocks[0].text, "write");
    }

    #[test]
    fn footnote_references_link_to_numbered_footnotes() {
        let blocks = blocks("Text[^note] and more[^b].\n\n[^b]: Second.\n[^note]: First.\n");
        let reference = blocks[0].runs.iter().find(|run| run.text == "[1]").unwrap();
        assert!(reference.style.sup);
        assert_eq!(reference.style.link.as_deref(), Some("#fn:1"));
        assert!(blocks.iter().any(|block| block.kind == Kind::Footnote(1) && block.text == "First."));
        assert!(blocks.iter().any(|block| block.kind == Kind::Footnote(2) && block.text == "Second."));
    }

    #[test]
    fn definition_lists_have_terms_and_definitions() {
        let blocks = blocks("Rope\n: A tree of strings.\n");
        assert_eq!(blocks[0].kind, Kind::Term);
        assert_eq!(blocks[0].text, "Rope");
        assert_eq!(blocks[1].kind, Kind::Definition);
        assert_eq!(blocks[1].text, "A tree of strings.");
    }

    #[test]
    fn links_keep_their_destination_and_a_lone_image_is_an_image_block() {
        let blocks = blocks("See [the docs](docs/a.md).\n\n![Diagram](img/flow.png)\n\nInline ![x](y.png) image.\n");
        let link = blocks[0].runs.iter().find(|run| run.text == "the docs").unwrap();
        assert_eq!(link.style.link.as_deref(), Some("docs/a.md"));
        assert_eq!(blocks[1].kind, Kind::Image);
        assert_eq!(blocks[1].src.as_deref(), Some("img/flow.png"));
        assert_eq!(blocks[1].text, "Diagram");
        assert_eq!(blocks[2].kind, Kind::Paragraph);
        // An image inside a paragraph keeps its own run, with its alt text and path.
        let image = blocks[2].runs.iter().find(|run| run.style.link.as_deref() == Some("image:y.png")).unwrap();
        assert_eq!(image.text, "x");
        assert_eq!(plain_text(&blocks[2].runs), "Inline x image.");
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
    fn inline_math_follows_the_apps_rules_so_prices_stay_text() {
        // The app sample's own line.
        let blocks = blocks("Inline $E = mc^2$ and $\\frac{n}{2}$, while prices like $5 and $10 stay text.\n");
        let math: Vec<&str> = blocks[0].runs.iter().filter(|run| run.style.math).map(|run| run.text.as_str()).collect();
        assert_eq!(math, ["E = mc^2", "\\frac{n}{2}"]);
        assert!(blocks[0].text.ends_with("prices like $5 and $10 stay text."));
    }

    #[test]
    fn a_paragraph_of_display_math_is_a_math_block() {
        let blocks = blocks("$$\n\\sum_j e^{x_j}\n$$\n\nText with $$x$$ inside.\n");
        assert_eq!(blocks[0].kind, Kind::Math);
        assert_eq!(blocks[0].text, "\\sum_j e^{x_j}");
        assert_eq!(blocks[1].kind, Kind::Paragraph);
        assert!(blocks[1].runs.iter().any(|run| run.style.display && run.text == "x"));
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
    fn line_numbers_stay_right_across_many_blocks() {
        let markdown: String = (0..2000).map(|index| format!("Paragraph {index}.\n\n")).collect();
        let blocks = blocks(&markdown);
        assert_eq!(blocks.len(), 2000);
        assert!(blocks.iter().enumerate().all(|(index, block)| block.line == index * 2 + 1));
    }

    #[test]
    fn block_at_line_uses_the_latest_block_at_or_above_the_line() {
        let blocks = blocks("# Title\n\nHello there.\n\n## Next\n");
        assert_eq!(block_at_line(&blocks, 1), 0);
        assert_eq!(block_at_line(&blocks, 4), 1);
        assert_eq!(block_at_line(&blocks, 5), 2);
    }

    #[test]
    fn footnotes_are_gathered_at_the_end_under_a_rule() {
        let markdown = "One[^b] and two[^a].\n\n[^a]: First note.\n\nMiddle.\n\n[^b]: Second note.\n\n[^unused]: Never shown.\n";
        let blocks = blocks(markdown);
        let kinds: Vec<Kind> = blocks.iter().map(|block| block.kind).collect();
        assert_eq!(kinds, vec![Kind::Paragraph, Kind::Paragraph, Kind::Rule, Kind::Footnote(1), Kind::Footnote(2)]);
        // Numbered by first reference, each with a link back to it, at its own source line.
        assert_eq!(blocks[3].text, "Second note.");
        assert_eq!(blocks[3].line, 7);
        assert_eq!(blocks[3].runs.last().and_then(|run| run.style.link.as_deref()), Some("#fnref:1"));
    }

    #[test]
    fn rules_abbreviations_and_display_math_follow_the_app() {
        let markdown = "Above\n\n---\n\n*[HTML]: Hyper Text Markup Language\n\nHTML here.\n\n$$\n\\frac{1}{\n$$\n";
        let blocks = blocks(markdown);
        let kinds: Vec<Kind> = blocks.iter().map(|block| block.kind).collect();
        assert_eq!(kinds, vec![Kind::Paragraph, Kind::Rule, Kind::Paragraph, Kind::Math]);
        assert_eq!(blocks[2].runs[0].style.abbr.as_deref(), Some("Hyper Text Markup Language"));
        assert_eq!(blocks[3].text, r"\frac{1}{");
    }

    #[test]
    fn the_typographer_setting_changes_punctuation() {
        let plain = blocks_with("\"Quotes\" -- and (c) 2026...", Flavor { typographer: false });
        let smart = blocks_with("\"Quotes\" -- and (c) 2026...", Flavor { typographer: true });
        assert_eq!(plain[0].text, "\"Quotes\" -- and (c) 2026...");
        assert_eq!(smart[0].text, "“Quotes” – and © 2026…");
    }

    #[test]
    fn bare_links_and_unbalanced_inline_math_become_runs() {
        let blocks = blocks(r"Visit https://example.com today, $\frac{1}{$ stays math.");
        let runs = &blocks[0].runs;
        assert!(runs.iter().any(|run| run.style.link.as_deref() == Some("https://example.com")));
        assert!(runs.iter().any(|run| run.style.math && run.text == r"\frac{1}{"));
    }
}
