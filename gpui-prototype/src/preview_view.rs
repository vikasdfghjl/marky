//! Draws the preview's blocks. Each block is one item of a virtualised GPUI list, so only the
//! blocks near the viewport are laid out, at any document size.

use gpui::{FontWeight, IntoElement, Pixels, SharedString, div, prelude::*, px, rgb};

use crate::preview::{self, Align, Block, Kind, Run, Table};

/// What every preview block is drawn with.
pub struct Look {
    pub font_size: f32,
    pub dark: bool,
    pub mono: SharedString,
}

impl Look {
    /// The app's `.markdown-body code, pre { font-size: 0.88em }`.
    fn code_size(&self) -> Pixels {
        px(self.font_size * 0.88)
    }

    fn border(&self) -> gpui::Rgba {
        tone(self.dark, 0x3f3f46, 0xd4d4d8)
    }
}

pub fn tone(dark: bool, dark_value: u32, light_value: u32) -> gpui::Rgba {
    if dark { rgb(dark_value) } else { rgb(light_value) }
}

/// One list item: the block with the pane's side padding, and the top padding on the first block.
pub fn preview_item(block: &Block, index: usize, look: &Look) -> gpui::AnyElement {
    div().px_4().when(index == 0, |item| item.pt_3()).child(preview_block(block, look)).into_any_element()
}

fn preview_block(block: &Block, look: &Look) -> gpui::Div {
    let dark = look.dark;
    let font_size = look.font_size;
    let size = match block.kind {
        Kind::Heading(1) => px(font_size + 8.),
        Kind::Heading(2) => px(font_size + 4.),
        Kind::Heading(_) => px(font_size + 2.),
        Kind::Code | Kind::Math | Kind::Diagram => look.code_size(),
        _ => px(font_size),
    };
    if let (Kind::Table, Some(table)) = (block.kind, &block.table) {
        return table_block(table, look);
    }
    let body = styled_runs(&block.runs, size, look);
    match block.kind {
        Kind::Quote => div().mb_3().pl_3().border_l_2().border_color(tone(dark, 0x71717a, 0xa1a1aa)).child(body),
        Kind::Item { ordered } => {
            let marker = match ordered {
                Some(number) => format!("{number}."),
                None => "•".to_string(),
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
            .child(div().text_size(px(12.)).text_color(tone(dark, 0xa1a1aa, 0x52525b)).child("Mermaid source"))
            .child(div().font_family(look.mono.clone()).child(body)),
        // Like the app: headings are bold, and h1 and h2 are underlined by a rule.
        Kind::Heading(level) => div()
            .mt_2()
            .mb_3()
            .font_weight(FontWeight::BOLD)
            .when(level <= 2, |heading| heading.pb_1().border_b_1().border_color(look.border()))
            .child(body),
        Kind::Paragraph => div().mb_3().child(body),
        Kind::Table => div(),
    }
}

/// The app's table: 1px borders, 6px 12px cell padding, a bold header row and shaded even body rows.
/// Columns get widths in proportion to their longest cell, so they line up across rows like an auto table.
fn table_block(table: &Table, look: &Look) -> gpui::Div {
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0).max(table.align.len());
    let weights: Vec<f32> = (0..columns)
        .map(|column| {
            let longest = table.rows.iter().filter_map(|row| row.get(column)).map(|cell| preview::plain_text(cell).chars().count()).max();
            longest.unwrap_or(0).max(3) as f32
        })
        .collect();
    let size = px(look.font_size);
    let shade = tone(look.dark, 0x18181b, 0xf4f4f5);
    let rows = table.rows.iter().enumerate().map(|(index, row)| {
        let cells = (0..columns).map(|column| {
            let runs = row.get(column).map(Vec::as_slice).unwrap_or(&[]);
            let mut cell = div()
                .min_w(px(0.))
                .flex_basis(px(0.))
                .px_3()
                .py(px(6.))
                .border_r_1()
                .border_b_1()
                .border_color(look.border())
                .child(styled_runs(runs, size, look));
            cell.style().flex_grow = Some(weights[column]);
            match table.align.get(column) {
                Some(Align::Center) => cell.text_center(),
                Some(Align::Right) => cell.text_right(),
                _ => cell,
            }
        });
        div()
            .flex()
            .flex_row()
            .w_full()
            .when(index == 0, |header| header.font_weight(FontWeight::BOLD))
            // Body rows count from 1, as in the app's `tr:nth-child(even)`.
            .when(index > 0 && index % 2 == 0, |row| row.bg(shade))
            .children(cells)
    });
    div().mb_3().w_full().flex().flex_col().border_t_1().border_l_1().border_color(look.border()).children(rows)
}

fn styled_runs(runs: &[Run], size: Pixels, look: &Look) -> gpui::Div {
    let dark = look.dark;
    let count = runs.len();
    let runs = runs.iter().map(|run| {
        let mut text = div().whitespace_normal().text_size(size).child(run.text.clone());
        if run.style.bold {
            text = text.font_weight(FontWeight::BOLD);
        }
        if run.style.italic {
            text = text.italic();
        }
        if run.style.strike {
            text = text.line_through();
        }
        if run.style.code {
            text = text.font_family(look.mono.clone()).text_size(look.code_size()).bg(tone(dark, 0x3f3f46, 0xe4e4e7)).px_1();
        }
        text
    });
    if count <= 1 {
        return div().w_full().children(runs);
    }
    div().w_full().flex().flex_row().flex_wrap().children(runs)
}

/// The range of blocks a re-parse changed, found like the app's preview-blocks.ts: the common prefix and
/// suffix are kept. Blocks compare without their line, so a line added above only renumbers kept blocks.
pub fn changed_blocks(old: &[Block], new: &[Block]) -> (std::ops::Range<usize>, usize) {
    let same = |a: &Block, b: &Block| a.kind == b.kind && a.text == b.text && a.runs == b.runs && a.table == b.table;
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
}
