//! Draws the preview's blocks. Each block is one item of a virtualised GPUI list, so only the
//! blocks near the viewport are laid out, at any document size.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{AppContext, ClickEvent, Context, FontWeight, IntoElement, Pixels, Role, SharedString, StyledText, SvgRenderer, div, img, prelude::*, px, rgb};

use crate::editor::{self, Prototype};
use crate::{diagrams, math};
use crate::preview::{self, Align, Block, Kind, Run, Table};
use crate::syntax::Span;

/// What every preview block is drawn with.
pub struct Look {
    pub font_size: f32,
    pub dark: bool,
    pub mono: SharedString,
    /// The open file's folder: relative images load from here, and only from inside it.
    pub folder: Option<PathBuf>,
    /// GPUI's SVG renderer, which draws formulas into bitmaps the preview owns (see `bitmaps.rs`).
    pub svg: SvgRenderer,
}

impl Look {
    /// The app's `.markdown-body code, pre { font-size: 0.88em }`.
    fn code_size(&self) -> Pixels {
        px(self.font_size * 0.88)
    }

    fn border(&self) -> gpui::Rgba {
        tone(self.dark, 0x3f3f46, 0xd4d4d8)
    }

    fn muted(&self) -> gpui::Rgba {
        tone(self.dark, 0xa1a1aa, 0x52525b)
    }
}

pub fn tone(dark: bool, dark_value: u32, light_value: u32) -> gpui::Rgba {
    if dark { rgb(dark_value) } else { rgb(light_value) }
}

/// One list item: the block with the pane's side padding, and the top padding on the first block.
/// Each block is a node for assistive technology with its role and its text, as the app's HTML is.
pub fn preview_item(block: &Block, index: usize, look: &Look, cx: &mut Context<Prototype>) -> gpui::AnyElement {
    div()
        .id(("block", index))
        .role(block_role(block.kind))
        .aria_label(SharedString::from(block_label(block)))
        .when_some(heading_level(block.kind), |item, level| item.aria_level(level))
        .px_4()
        .when(index == 0, |item| item.pt_3())
        .child(preview_block(block, look, cx))
        .into_any_element()
}

fn block_role(kind: Kind) -> Role {
    match kind {
        Kind::Heading(_) => Role::Heading,
        Kind::Paragraph => Role::Paragraph,
        Kind::Quote => Role::Blockquote,
        Kind::Item { .. } => Role::ListItem,
        Kind::Code => Role::Code,
        Kind::Math => Role::Math,
        Kind::Diagram | Kind::Image => Role::Image,
        Kind::Table => Role::Table,
        Kind::Footnote(_) => Role::DocFootnote,
        Kind::Term => Role::Term,
        Kind::Definition => Role::Definition,
        Kind::Rule => Role::Splitter,
    }
}

fn heading_level(kind: Kind) -> Option<usize> {
    match kind {
        Kind::Heading(level) => Some(level as usize),
        _ => None,
    }
}

/// What a screen reader reads for a block: its text, with a task's state, an image's alt text and
/// a formula's TeX (KaTeX's MathML would say more; RaTeX has no MathML).
fn block_label(block: &Block) -> String {
    match block.kind {
        Kind::Item { checked: Some(done), .. } => format!("{} {}", if done { "Done:" } else { "To do:" }, block.text),
        Kind::Image => format!("Image: {}", if block.text.is_empty() { "no description" } else { &block.text }),
        Kind::Diagram => "Mermaid diagram".to_string(),
        Kind::Math => format!("Formula: {}", block.text),
        Kind::Footnote(number) => format!("Footnote {number}: {}", block.text),
        Kind::Rule => String::new(),
        _ => block.text.clone(),
    }
}

fn preview_block(block: &Block, look: &Look, cx: &mut Context<Prototype>) -> gpui::Div {
    let dark = look.dark;
    let font_size = look.font_size;
    let size = match block.kind {
        Kind::Heading(1) => px(font_size + 8.),
        Kind::Heading(2) => px(font_size + 4.),
        Kind::Heading(_) => px(font_size + 2.),
        Kind::Code | Kind::Math | Kind::Diagram => look.code_size(),
        Kind::Footnote(_) => px(font_size * 0.9),
        _ => px(font_size),
    };
    if let (Kind::Table, Some(table)) = (block.kind, &block.table) {
        return table_block(table, look, cx);
    }
    if let (Kind::Code, Some(spans)) = (block.kind, &block.code) {
        return code_block(&block.text, spans, look);
    }
    if let Kind::Image = block.kind {
        return image_block(block, look);
    }
    if let Kind::Math = block.kind {
        return math_block(&block.text, look);
    }
    if let Kind::Diagram = block.kind {
        return diagram_block(&block.text, look, cx);
    }
    let body = styled_runs(&block.runs, size, look, cx);
    match block.kind {
        Kind::Quote => div().mb_3().pl_3().border_l_2().border_color(tone(dark, 0x71717a, 0xa1a1aa)).child(body),
        Kind::Item { ordered, checked } => {
            // Task items show their box, read-only, as the app's task lists do.
            let marker = match (checked, ordered) {
                (Some(true), _) => "☑".to_string(),
                (Some(false), _) => "☐".to_string(),
                (None, Some(number)) => format!("{number}."),
                (None, None) => "•".to_string(),
            };
            div().mb_1().flex().flex_row().gap_2().child(marker).child(body)
        }
        Kind::Code => div().mb_3().px_3().py_2().font_family(look.mono.clone()).bg(tone(dark, 0x27272a, 0xf4f4f5)).child(body),
        // Like the app: headings are bold, and h1 and h2 are underlined by a rule.
        Kind::Heading(level) => div()
            .mt_2()
            .mb_3()
            .font_weight(FontWeight::BOLD)
            .when(level <= 2, |heading| heading.pb_1().border_b_1().border_color(look.border()))
            .child(body),
        Kind::Footnote(number) => div()
            .mb_2()
            .flex()
            .flex_row()
            .gap_2()
            .text_color(look.muted())
            .child(div().text_size(size).child(format!("{number}.")))
            .child(body),
        Kind::Term => div().mt_1().font_weight(FontWeight::BOLD).child(body),
        Kind::Definition => div().mb_2().pl_6().child(body),
        Kind::Paragraph => div().mb_3().child(body),
        // The app's `hr`: a 1px line with the block spacing around it.
        Kind::Rule => div().mt_1().mb_4().h(px(1.)).bg(look.border()),
        Kind::Table | Kind::Image | Kind::Math | Kind::Diagram => div(),
    }
}

/// A fenced block in a language tree-sitter knows, in the editor's colours (the app's `tok-*` → `--hl-*`).
fn code_block(text: &str, spans: &[Span], look: &Look) -> gpui::Div {
    let highlights = spans.iter().map(|(range, token)| (range.clone(), token.style(look.dark)));
    div()
        .mb_3()
        .px_3()
        .py_2()
        .font_family(look.mono.clone())
        .text_size(look.code_size())
        .bg(tone(look.dark, 0x27272a, 0xf4f4f5))
        .child(StyledText::new(text.to_string()).with_highlights(highlights))
}

/// A display formula, centred like KaTeX's display mode. Broken TeX shows its source and the error,
/// as the app does with KaTeX's `throwOnError: false`.
fn math_block(tex: &str, look: &Look) -> gpui::Div {
    match math::render(tex, true, look.font_size, look.dark, &look.svg) {
        Ok(formula) => div().mb_3().flex().justify_center().child(formula_image(&formula)),
        Err(err) => div()
            .mb_3()
            .px_3()
            .py_2()
            .font_family(look.mono.clone())
            .text_size(look.code_size())
            .bg(tone(look.dark, 0x27272a, 0xf4f4f5))
            .child(tex.to_string())
            .child(div().pt_1().text_size(px(12.)).text_color(error_color(look.dark)).child(format!("TeX error: {err}"))),
    }
}

/// A Mermaid diagram, centred and never wider than drawn, like the app's (`width="100%"` with a
/// `max-width`): it shrinks with the pane and keeps its shape. While the helper draws it, its
/// source shows in its place; a broken diagram shows the error as the app does (`.mermaid-error`).
fn diagram_block(source: &str, look: &Look, cx: &mut Context<Prototype>) -> gpui::Div {
    match diagrams::state(source, look.dark, cx) {
        // The box keeps the shape: given only a relative width, GPUI's img takes the image's own
        // height, so a shrunk diagram would float in a box as tall as the unshrunk one.
        diagrams::State::Drawn(diagram) => div().mb_3().flex().justify_center().child(
            div()
                .w_full()
                .max_w(px(diagram.width))
                .aspect_ratio(diagram.width / diagram.height)
                .child(img(diagram.image.clone()).size_full()),
        ),
        diagrams::State::Drawing => {
            div().mb_3().px_3().py_2().font_family(look.mono.clone()).text_size(look.code_size()).text_color(look.muted()).child(source.to_string())
        }
        diagrams::State::Failed(err) => div()
            .mb_3()
            .px_3()
            .py_2()
            .border_l_4()
            .border_color(error_color(look.dark))
            .font_family(look.mono.clone())
            .text_size(px(look.font_size * 0.85))
            .child(format!("Mermaid: {err}")),
    }
}

/// An inline formula in a paragraph; broken TeX stays as its `$…$` source in the error colour.
fn inline_formula(run: &Run, look: &Look) -> gpui::Div {
    match math::render(&run.text, run.style.display, look.font_size, look.dark, &look.svg) {
        Ok(formula) => div().child(formula_image(&formula)),
        Err(_) => {
            let fence = if run.style.display { "$$" } else { "$" };
            div().text_color(error_color(look.dark)).child(format!("{fence}{}{fence}", run.text))
        }
    }
}

fn formula_image(formula: &math::Formula) -> gpui::Img {
    img(formula.image.clone()).w(px(formula.width)).h(px(formula.height))
}

fn error_color(dark: bool) -> gpui::Rgba {
    tone(dark, 0xfca5a5, 0xb91c1c)
}

/// The app loads relative images from the document's folder and blocks remote ones (its CSP allows
/// no network requests). The prototype does the same: anything else shows its alt text.
fn image_block(block: &Block, look: &Look) -> gpui::Div {
    let alt = if block.text.is_empty() { "image".to_string() } else { block.text.clone() };
    let source = block.src.as_deref().unwrap_or_default();
    let Some(path) = local_image(source, look.folder.as_deref()) else {
        let reason = if source.contains("://") { "remote images are not loaded" } else { "not found next to this file" };
        return div().mb_3().italic().text_color(look.muted()).child(format!("[{alt}: {reason}]"));
    };
    div().mb_3().child(img(path).max_w_full())
}

/// A relative path that resolves inside `folder` and exists. URLs and paths that leave the folder are refused.
pub fn local_image(source: &str, folder: Option<&Path>) -> Option<PathBuf> {
    let folder = folder?;
    if source.is_empty() || source.contains("://") || source.starts_with("data:") || Path::new(source).is_absolute() {
        return None;
    }
    let decoded = source.split(['#', '?']).next()?.replace("%20", " ");
    let path = folder.join(decoded).canonicalize().ok()?;
    let folder = folder.canonicalize().ok()?;
    path.starts_with(&folder).then_some(path)
}

/// The app's table: 1px borders, 6px 12px cell padding, a bold header row and shaded even body rows.
/// Columns get widths in proportion to their longest cell, so they line up across rows like an auto table.
fn table_block(table: &Table, look: &Look, cx: &mut Context<Prototype>) -> gpui::Div {
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0).max(table.align.len());
    let weights: Vec<f32> = (0..columns)
        .map(|column| {
            let longest = table.rows.iter().filter_map(|row| row.get(column)).map(|cell| preview::plain_text(cell).chars().count()).max();
            longest.unwrap_or(0).max(3) as f32
        })
        .collect();
    let size = px(look.font_size);
    let shade = tone(look.dark, 0x18181b, 0xf4f4f5);
    let mut rows = Vec::with_capacity(table.rows.len());
    for (index, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(columns);
        for (column, weight) in weights.iter().enumerate() {
            let runs = row.get(column).map(Vec::as_slice).unwrap_or(&[]);
            let mut cell = div()
                .min_w(px(0.))
                .flex_basis(px(0.))
                .px_3()
                .py(px(6.))
                .border_r_1()
                .border_b_1()
                .border_color(look.border())
                .child(styled_runs(runs, size, look, cx));
            cell.style().flex_grow = Some(*weight);
            cells.push(match table.align.get(column) {
                Some(Align::Center) => cell.text_center(),
                Some(Align::Right) => cell.text_right(),
                _ => cell,
            });
        }
        rows.push(
            div()
                .flex()
                .flex_row()
                .w_full()
                .when(index == 0, |header| header.font_weight(FontWeight::BOLD))
                // Body rows count from 1, as in the app's `tr:nth-child(even)`.
                .when(index > 0 && index % 2 == 0, |row| row.bg(shade))
                .children(cells),
        );
    }
    div().mb_3().w_full().flex().flex_col().border_t_1().border_l_1().border_color(look.border()).children(rows)
}

fn styled_runs(runs: &[Run], size: Pixels, look: &Look, cx: &mut Context<Prototype>) -> gpui::Div {
    let count = runs.len();
    // Each piece is a box in a wrapping row, and a box wraps whole. So when a paragraph has more than
    // one piece (styles, links, formulas, images), plain text goes in word by word and flows like
    // text; a link or an abbreviation stays one piece, one node for assistive technology.
    let has_boxes = runs.iter().any(|run| run.style.math || image_source(run).is_some());
    let by_word = has_boxes || count > 1;
    let mut pieces: Vec<gpui::AnyElement> = Vec::with_capacity(count);
    for (index, run) in runs.iter().enumerate() {
        if run.style.math {
            pieces.push(inline_formula(run, look).into_any_element());
        } else if let Some(source) = image_source(run) {
            pieces.push(inline_image(run, source, look).into_any_element());
        } else if by_word && run.style.link.is_none() && run.style.abbr.is_none() {
            for (word, text) in run.text.split_inclusive(' ').enumerate() {
                pieces.push(styled_piece(text, run, size, look, Some(word), index, cx));
            }
        } else {
            pieces.push(styled_piece(&run.text, run, size, look, None, index, cx));
        }
    }
    if count <= 1 && !has_boxes {
        return div().w_full().children(pieces);
    }
    // Formulas and images are taller than the text around them; centre each line's pieces on one another.
    div().w_full().flex().flex_row().flex_wrap().when(has_boxes, |row| row.items_center()).children(pieces)
}

/// An image inside a paragraph: its path when the run is one (`image:` destinations).
fn image_source(run: &Run) -> Option<&str> {
    run.style.link.as_deref()?.strip_prefix("image:")
}

/// An image in the flow of a paragraph, from the document's folder only, never wider than the pane.
/// Anything else shows its alt text, as the image blocks do.
fn inline_image(run: &Run, source: &str, look: &Look) -> gpui::Div {
    match local_image(source, look.folder.as_deref()) {
        Some(path) => div().max_w_full().child(img(path).max_w_full()),
        None => div().italic().text_color(look.muted()).child(format!("[{}]", if run.text.is_empty() { "image" } else { &run.text })),
    }
}

/// One run of text, or one word of it (`word`), with the run's style. Links and abbreviations are
/// nodes of their own for assistive technology; `index` keeps their ids apart within the block.
#[allow(clippy::too_many_arguments)]
fn styled_piece(text: &str, run: &Run, size: Pixels, look: &Look, word: Option<usize>, index: usize, cx: &mut Context<Prototype>) -> gpui::AnyElement {
    let dark = look.dark;
    let style = &run.style;
    // Sub- and superscript are smaller and shifted, like the app's 80% and ±0.3em.
    let run_size = if style.sub || style.sup { size * 0.8 } else { size };
    let mut piece = div()
        .map(|piece| if word.is_some() { piece.whitespace_nowrap() } else { piece.whitespace_normal() })
        .text_size(run_size)
        .child(text.to_string());
    if style.bold {
        piece = piece.font_weight(FontWeight::BOLD);
    }
    if style.italic {
        piece = piece.italic();
    }
    if style.strike {
        piece = piece.line_through();
    }
    if style.sup {
        piece = piece.relative().top(-size * 0.3);
    }
    if style.sub {
        piece = piece.relative().top(size * 0.25);
    }
    if style.mark {
        // The app's --mark.
        piece = piece.bg(tone(dark, 0x4b3218, 0xffeed8));
    }
    if style.code {
        piece = piece.font_family(look.mono.clone()).text_size(look.code_size()).bg(tone(dark, 0x3f3f46, 0xe4e4e7)).px_1();
    }
    let id = ("piece", index << 16 | word.unwrap_or(0));
    if let Some(link) = style.link.clone() {
        return clickable(piece.id(id), text, link, dark, cx).into_any_element();
    }
    if let Some(title) = style.abbr.clone() {
        return abbreviation(piece.id(id), text, title, look).into_any_element();
    }
    piece.into_any_element()
}

/// A link in the app's link colour, reachable with Tab like the app's `<a>`. Clicks, Enter and a
/// screen reader's "click" go to `Prototype::follow_link`, which applies the app's rules.
fn clickable(piece: gpui::Stateful<gpui::Div>, text: &str, link: Arc<str>, dark: bool, cx: &mut Context<Prototype>) -> gpui::Stateful<gpui::Div> {
    let ring = editor::ring(dark);
    piece
        .role(Role::Link)
        .aria_label(SharedString::from(text.trim().to_string()))
        .focusable()
        .tab_stop(true)
        .focus_visible(move |style| style.border_1().border_color(ring))
        .text_color(tone(dark, 0x58a6ff, 0x0969da))
        .underline()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.follow_link(&link, window, cx)))
}

/// An abbreviation, underlined, with its title on hover and for screen readers (the app's `<abbr title>`).
fn abbreviation(piece: gpui::Stateful<gpui::Div>, text: &str, title: Arc<str>, look: &Look) -> gpui::Stateful<gpui::Div> {
    let dark = look.dark;
    let label = SharedString::from(format!("{} ({title})", text.trim()));
    let tip: SharedString = SharedString::from(title.to_string());
    piece.role(Role::Abbr).aria_label(label).underline().text_decoration_color(look.muted()).tooltip(move |_, cx| {
        let tip = tip.clone();
        cx.new(|_| TitleTip { text: tip, dark }).into()
    })
}

/// The hover tooltip for an abbreviation's title.
struct TitleTip {
    text: SharedString,
    dark: bool,
}

impl gpui::Render for TitleTip {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .text_size(px(12.))
            .bg(tone(self.dark, 0x3f3f46, 0xffffff))
            .text_color(tone(self.dark, 0xfafafa, 0x18181b))
            .border_1()
            .border_color(tone(self.dark, 0x52525b, 0xd4d4d8))
            .child(self.text.clone())
    }
}

/// The range of blocks a re-parse changed, found like the app's preview-blocks.ts: the common prefix and
/// suffix are kept. Blocks compare without their line, so a line added above only renumbers kept blocks.
pub fn changed_blocks(old: &[Block], new: &[Block]) -> (std::ops::Range<usize>, usize) {
    let same = |a: &Block, b: &Block| a.kind == b.kind && a.text == b.text && a.runs == b.runs && a.table == b.table && a.code == b.code && a.src == b.src;
    let prefix = old.iter().zip(new).take_while(|(a, b)| same(a, b)).count();
    let suffix = old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| same(a, b)).count();
    (prefix..old.len() - suffix, new.len() - prefix - suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_edited_block_changes_when_a_line_is_added_above() {
        let old = preview::blocks("# A\n\nOne.\n\nTwo.\n");
        let new = preview::blocks("# A\n\nOne, edited.\n\n\nTwo.\n");
        assert_eq!(changed_blocks(&old, &new), (1..2, 1));
        assert_eq!(changed_blocks(&old, &old), (3..3, 0));
    }

    #[test]
    fn images_load_only_from_inside_the_documents_folder() {
        let dir = std::env::temp_dir().join(format!("marky-gpui-images-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img").join("a b.png"), b"png").unwrap();
        assert!(local_image("img/a%20b.png", Some(&dir)).is_some());
        assert!(local_image("img/missing.png", Some(&dir)).is_none());
        assert!(local_image("https://example.com/a.png", Some(&dir)).is_none());
        assert!(local_image("../outside.png", Some(&dir)).is_none());
        assert!(local_image("img/a%20b.png", None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
