//! Draws the preview's blocks. Each block is one item of a virtualised GPUI list, so only the
//! blocks near the viewport are laid out, at any document size.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, FontWeight, IntoElement, MouseButton, Pixels, SharedString, StyledText, div, img, prelude::*, px, rgb};

use crate::editor::Prototype;
use crate::preview::{self, Align, Block, Kind, Run, Table};
use crate::syntax::Span;

/// What every preview block is drawn with.
pub struct Look {
    pub font_size: f32,
    pub dark: bool,
    pub mono: SharedString,
    /// The open file's folder: relative images load from here, and only from inside it.
    pub folder: Option<PathBuf>,
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
pub fn preview_item(block: &Block, index: usize, look: &Look, cx: &mut Context<Prototype>) -> gpui::AnyElement {
    div().px_4().when(index == 0, |item| item.pt_3()).child(preview_block(block, look, cx)).into_any_element()
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
        Kind::Math => div()
            .mb_3()
            .px_3()
            .py_2()
            .font_family(look.mono.clone())
            .bg(tone(dark, 0x1e1b4b, 0xeef2ff))
            .text_color(tone(dark, 0xe0e7ff, 0x312e81))
            .child(body),
        Kind::Diagram => div()
            .mb_3()
            .px_3()
            .py_2()
            .border_1()
            .border_color(tone(dark, 0x52525b, 0xa1a1aa))
            .child(div().text_size(px(12.)).text_color(look.muted()).child("Mermaid source"))
            .child(div().font_family(look.mono.clone()).child(body)),
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
        Kind::Table | Kind::Image => div(),
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
    let dark = look.dark;
    let count = runs.len();
    let mut pieces = Vec::with_capacity(count);
    for run in runs {
        let style = &run.style;
        // Sub- and superscript are smaller and shifted, like the app's 80% and ±0.3em.
        let run_size = if style.sub || style.sup { size * 0.8 } else { size };
        let mut text = div().whitespace_normal().text_size(run_size).child(run.text.clone());
        if style.bold {
            text = text.font_weight(FontWeight::BOLD);
        }
        if style.italic {
            text = text.italic();
        }
        if style.strike {
            text = text.line_through();
        }
        if style.sup {
            text = text.relative().top(-size * 0.3);
        }
        if style.sub {
            text = text.relative().top(size * 0.25);
        }
        if style.mark {
            // The app's --mark.
            text = text.bg(tone(dark, 0x4b3218, 0xffeed8));
        }
        if style.code {
            text = text.font_family(look.mono.clone()).text_size(look.code_size()).bg(tone(dark, 0x3f3f46, 0xe4e4e7)).px_1();
        }
        // Image alt text inside a paragraph is not a link; everything else with a destination is.
        if let Some(link) = style.link.clone().filter(|link| !link.starts_with("image:")) {
            text = clickable(text, link, dark, cx);
        }
        pieces.push(text);
    }
    if count <= 1 {
        return div().w_full().children(pieces);
    }
    div().w_full().flex().flex_row().flex_wrap().children(pieces)
}

/// A link in the app's link colour. Clicks go to `Prototype::follow_link`, which applies the app's rules.
fn clickable(text: gpui::Div, link: Arc<str>, dark: bool, cx: &mut Context<Prototype>) -> gpui::Div {
    text.text_color(tone(dark, 0x58a6ff, 0x0969da))
        .underline()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.follow_link(&link, window, cx)))
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
