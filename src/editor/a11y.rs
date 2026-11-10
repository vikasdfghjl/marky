//! The editor's text for assistive technology. GPUI draws text itself, so a screen reader sees
//! nothing of it unless the editor describes it: here, as AccessKit text runs under the editor's
//! `MultilineTextInput` node, with the caret and selection as a text selection. On Windows that is
//! UI Automation's TextPattern, which screen readers use to read by character, word and line and to
//! follow the caret.
//!
//! Only the lines on screen and the caret's line are described, as CodeMirror keeps only the visible
//! lines in the DOM: describing a 4 MB document on every frame would cost more than drawing it. The
//! work is skipped entirely while no assistive technology is listening.

use std::hash::Hash;

use gpui::A11ySubtreeBuilder;
use gpui::accesskit::{Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection};

/// AccessKit stores word starts as `u8` offsets into a run, so a run holds at most this many characters.
const RUN_CHARS: usize = 255;

/// Lines described below the top visible one, and a cap on characters, so a few very long lines
/// cannot make the tree huge.
pub const LINES_DESCRIBED: usize = 120;
pub const CHARS_DESCRIBED: usize = 60_000;

/// One source line as described: its index and its text, with a trailing `\n` unless it is the last
/// line of the document (AccessKit marks line ends with the line break character).
pub struct Line {
    pub index: usize,
    pub text: String,
    /// The line's painted box, in physical pixels. A screen magnifier uses it to follow the caret;
    /// a line that was not painted this frame has none.
    pub bounds: Option<Rect>,
    /// Each character's x offset from the line's left edge and its advance, in physical pixels.
    /// AccessKit needs both to turn a caret position into a screen rectangle.
    pub positions: Option<Vec<f32>>,
    pub widths: Option<Vec<f32>>,
}

/// A caret or selection end: a line and a character (not byte) index within it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub line: usize,
    pub char_index: usize,
}

/// Which lines to describe: those from the top of the view, then the caret's and the selection
/// anchor's lines if they are elsewhere. Sorted and without duplicates.
pub fn lines_to_describe(top: usize, line_count: usize, caret_line: usize, anchor_line: usize) -> Vec<usize> {
    let end = (top + LINES_DESCRIBED).min(line_count);
    let mut lines: Vec<usize> = (top.min(end)..end).collect();
    for extra in [caret_line, anchor_line] {
        if extra < line_count && !lines.contains(&extra) {
            lines.push(extra);
        }
    }
    lines.sort_unstable();
    lines
}

/// Keeps lines until the character budget is spent, always keeping the lines with the selection.
pub fn within_budget(lines: Vec<Line>, keep: [usize; 2]) -> Vec<Line> {
    let mut spent = 0;
    lines
        .into_iter()
        .filter(|line| {
            let must = keep.contains(&line.index);
            if !must && spent >= CHARS_DESCRIBED {
                return false;
            }
            spent += line.text.len();
            true
        })
        .collect()
}

/// The runs for `lines` and the selection between `anchor` and `focus`, with node ids from `id_of`
/// (a line and chunk number). Pure, so it can be tested without a window.
pub fn build_runs(lines: &[Line], anchor: Point, focus: Point, id_of: impl Fn((usize, usize)) -> NodeId) -> (Vec<(NodeId, Node)>, Option<TextSelection>) {
    let mut runs = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.text.chars().collect();
        let chunks = chars.len().div_ceil(RUN_CHARS).max(1);
        let mut in_word = false;
        for chunk in 0..chunks {
            let part = &chars[(chunk * RUN_CHARS).min(chars.len())..((chunk + 1) * RUN_CHARS).min(chars.len())];
            let mut word_starts = Vec::new();
            for (offset, ch) in part.iter().enumerate() {
                let word = ch.is_alphanumeric() || *ch == '_';
                if word && !in_word {
                    word_starts.push(offset as u8);
                }
                in_word = word;
            }
            let mut node = Node::new(Role::TextRun);
            node.set_text_direction(TextDirection::LeftToRight);
            node.set_value(part.iter().collect::<String>());
            node.set_character_lengths(part.iter().map(|ch| ch.len_utf8() as u8).collect::<Vec<u8>>());
            node.set_word_starts(word_starts);
            if let Some(bounds) = line.bounds {
                node.set_bounds(bounds);
            }
            let start = chunk * RUN_CHARS;
            if let (Some(positions), Some(widths)) = (&line.positions, &line.widths) {
                let end = (start + part.len()).min(positions.len().min(widths.len()));
                if start < end {
                    node.set_character_positions(positions[start..end].to_vec().into_boxed_slice());
                    node.set_character_widths(widths[start..end].to_vec().into_boxed_slice());
                }
            }
            if chunk > 0 {
                node.set_previous_on_line(id_of((line.index, chunk - 1)));
            }
            if chunk + 1 < chunks {
                node.set_next_on_line(id_of((line.index, chunk + 1)));
            }
            runs.push((id_of((line.index, chunk)), node));
        }
    }
    let position = |point: Point| -> Option<TextPosition> {
        let line = lines.iter().find(|line| line.index == point.line)?;
        let length = line.text.chars().count();
        let char_index = point.char_index.min(length);
        // A position on a chunk boundary belongs to the end of the earlier chunk.
        let chunk = if char_index > 0 && char_index % RUN_CHARS == 0 { char_index / RUN_CHARS - 1 } else { char_index / RUN_CHARS };
        Some(TextPosition { node: id_of((point.line, chunk)), character_index: char_index - chunk * RUN_CHARS })
    };
    let selection = match (position(anchor), position(focus)) {
        (Some(anchor), Some(focus)) => Some(TextSelection { anchor, focus }),
        (None, Some(focus)) => Some(TextSelection { anchor: focus, focus }),
        _ => None,
    };
    (runs, selection)
}

/// Adds the runs and the selection to the editor's node. Bounds are filled in from the lines painted
/// this frame, so a magnifier can follow the caret.
pub fn describe(builder: &mut A11ySubtreeBuilder, lines: &[Line], anchor: Point, focus: Point) {
    let (runs, selection) = build_runs(lines, anchor, focus, |key: (usize, usize)| synthetic_id(builder, key));
    for (id, node) in runs {
        builder.push_child(id, node);
    }
    if let Some(selection) = selection {
        builder.parent_node().set_text_selection(selection);
    }
}

/// The preview's text as one read-only text pattern: a screen reader can review it by character,
/// word and line, which a set of block names does not allow.
pub fn describe_document(builder: &mut A11ySubtreeBuilder, lines: &[Line]) {
    let (runs, _) = build_runs(lines, Point { line: 0, char_index: 0 }, Point { line: 0, char_index: 0 }, |key: (usize, usize)| synthetic_id(builder, key));
    for (id, node) in runs {
        builder.push_child(id, node);
    }
}

fn synthetic_id(builder: &A11ySubtreeBuilder, key: impl Hash) -> NodeId {
    builder.synthetic_node_id(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id((line, chunk): (usize, usize)) -> NodeId {
        NodeId((line as u64) << 16 | chunk as u64)
    }

    fn line(index: usize, text: &str) -> Line {
        Line { index, text: text.to_string(), bounds: None, positions: None, widths: None }
    }

    #[test]
    fn each_line_is_a_run_with_its_words_and_the_caret() {
        let lines = [line(3, "# Hello world\n"), line(4, "é and ü\n")];
        let caret = Point { line: 4, char_index: 2 };
        let (runs, selection) = build_runs(&lines, caret, caret, id);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].1.value(), Some("# Hello world\n"));
        assert_eq!(runs[0].1.word_starts(), &[2, 8]);
        // "é" is two bytes in UTF-8.
        assert_eq!(runs[1].1.character_lengths()[0], 2);
        let selection = selection.unwrap();
        assert_eq!(selection.focus, TextPosition { node: id((4, 0)), character_index: 2 });
    }

    #[test]
    fn long_lines_are_split_and_linked() {
        let long = "a".repeat(600);
        let lines = [line(0, &long)];
        let end = Point { line: 0, char_index: 510 };
        let (runs, selection) = build_runs(&lines, Point { line: 0, char_index: 0 }, end, id);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].1.previous_on_line(), Some(id((0, 0))));
        assert_eq!(runs[1].1.next_on_line(), Some(id((0, 2))));
        // 510 is the end of the second chunk, not the start of the third.
        assert_eq!(selection.unwrap().focus, TextPosition { node: id((0, 1)), character_index: 255 });
    }

    #[test]
    fn the_caret_line_is_described_even_when_scrolled_away() {
        assert_eq!(lines_to_describe(0, 1000, 500, 500).last(), Some(&500));
        assert_eq!(lines_to_describe(0, 1000, 500, 500).len(), LINES_DESCRIBED + 1);
        assert_eq!(lines_to_describe(10, 12, 11, 2), vec![2, 10, 11]);
    }

    #[test]
    fn a_painted_line_gives_its_runs_bounds() {
        let bounds = Rect { x0: 10.0, y0: 20.0, x1: 110.0, y1: 35.0 };
        let mut line = line(0, "hello");
        line.bounds = Some(bounds);
        line.positions = Some(vec![0.0, 10.0, 20.0, 30.0, 40.0]);
        line.widths = Some(vec![10.0; 5]);
        let (runs, _) = build_runs(&[line], Point { line: 0, char_index: 0 }, Point { line: 0, char_index: 0 }, id);
        assert_eq!(runs[0].1.bounds(), Some(bounds));
        assert_eq!(runs[0].1.character_positions(), Some(&[0.0, 10.0, 20.0, 30.0, 40.0][..]));
        assert_eq!(runs[0].1.character_widths(), Some(&[10.0; 5][..]));
    }

    #[test]
    fn an_empty_document_still_has_one_run() {
        let (runs, selection) = build_runs(&[line(0, "")], Point { line: 0, char_index: 0 }, Point { line: 0, char_index: 0 }, id);
        assert_eq!(runs.len(), 1);
        assert!(selection.is_some());
    }
}
