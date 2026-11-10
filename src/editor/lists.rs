//! Enter in a Markdown list continues it, as the app's CodeMirror editor did
//! (`insertNewlineContinueMarkup`): after `2. Oranges` the new line starts `3. `, after `- a` it
//! starts `- `, and after a task it starts with an unchecked box. Enter on an item with nothing in
//! it ends the list instead. Items after the caret that continue its numbering are renumbered.
//!
//! Markers follow CommonMark's list-item rule: a bullet (`-`, `+`, `*`) or one to nine digits and
//! `.` or `)`, then a space or the end of the line. Only the caret's line and the lines after it
//! are read, so this works mid-burst, before the background parse has caught up with the text.

use std::fmt::Write as _;
use std::ops::Range;

use crate::buffer::Buffer;

/// Lines after the caret's item that one Enter looks through to renumber; longer lists keep the
/// numbers past it, which Markdown renders in sequence anyway.
const RENUMBER_LINES: usize = 10_000;

/// The edit Enter makes in a list item, and where the caret goes after it.
#[derive(Debug, PartialEq, Eq)]
pub struct ListEnter {
    pub range: Range<usize>,
    pub text: String,
    pub caret: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Bullet(char),
    /// `width` is the number's zero-padded width (`01.`), or 0 when it is not padded.
    Ordered { number: u64, width: usize, delimiter: char },
}

impl Marker {
    fn next(self) -> Self {
        match self {
            Marker::Ordered { number, width, delimiter } => Marker::Ordered { number: number + 1, width, delimiter },
            bullet => bullet,
        }
    }

    fn write_to(self, out: &mut String) {
        match self {
            Marker::Bullet(bullet) => out.push(bullet),
            Marker::Ordered { number, width, delimiter } => {
                let _ = write!(out, "{number:0width$}{delimiter}");
            }
        }
    }
}

/// A list item's line, in parts: `indent`, the marker, `gap`, an optional task box, then the text.
struct Item<'a> {
    indent: &'a str,
    marker: Marker,
    /// Where the marker ends and the gap begins.
    marker_end: usize,
    gap: &'a str,
    task: bool,
    /// Where the item's own text starts, after the gap and any task box.
    text_start: usize,
}

impl Item<'_> {
    /// Where the caret has to be for Enter to continue the list: after the marker and its gap.
    fn content_start(&self) -> usize {
        self.marker_end + self.gap.len()
    }
}

/// What Enter does at `caret` when it is in a list item; `None` for a plain new line.
pub fn on_enter(buffer: &Buffer, caret: usize) -> Option<ListEnter> {
    let index = buffer.line_of(caret);
    let start = buffer.line_start(index);
    let line = buffer.line(index);
    let item = parse_item(&line)?;
    let column = caret - start;
    // In the indent or on the marker itself, Enter only breaks the line, as in CodeMirror.
    if column < item.content_start() {
        return None;
    }
    let after_caret = &line[column..];
    if line[item.text_start..].trim().is_empty() && after_caret.trim().is_empty() {
        // An empty item ends the list: the marker goes and the line is left blank.
        return Some(ListEnter { range: start..start + line.len(), text: String::new(), caret: start });
    }
    let next = item.marker.next();
    let mut text = format!("\n{}", item.indent);
    next.write_to(&mut text);
    text.push_str(if item.gap.is_empty() { " " } else { item.gap });
    if item.task {
        text.push_str("[ ] ");
    }
    let caret_after = caret + text.len();
    // Text moved into the new item starts at its first character, not at the space the caret was before.
    let moved = after_caret.trim_start_matches([' ', '\t']);
    let mut end = caret + (after_caret.len() - moved.len());
    if let Some((renumbered, through)) = renumber_following(buffer, index, &item) {
        text.push_str(moved);
        text.push_str(&renumbered);
        end = buffer.line_range(through).end;
    }
    Some(ListEnter { range: caret..end, text, caret: caret_after })
}

/// The ordered items after line `index` that continue its numbering, each moved up by one to make
/// room for the item Enter inserts, with the lines between them (nested items, continuation lines,
/// single blank lines) kept as they are. Returns the replacement for everything after `index`'s
/// line up to the last renumbered item, and that item's line; `None` when nothing needs renumbering.
fn renumber_following(buffer: &Buffer, index: usize, item: &Item) -> Option<(String, usize)> {
    let Marker::Ordered { number, delimiter, .. } = item.marker else { return None };
    let mut expected = number + 1;
    let (mut out, mut between, mut through) = (String::new(), String::new(), None);
    let mut blank_before = false;
    let last = buffer.line_count().min(index + 1 + RENUMBER_LINES);
    for line_index in index + 1..last {
        let line = buffer.line(line_index);
        let blank = line.trim().is_empty();
        // Two blank lines in a row end a list in practice; stop rather than reach into the next one.
        if blank && blank_before {
            break;
        }
        blank_before = blank;
        let following = parse_item(&line).filter(|following| following.indent == item.indent);
        match following.map(|following| (following.marker, following.marker_end)) {
            Some((Marker::Ordered { number: found, width, delimiter: found_delimiter }, marker_end)) if found == expected && found_delimiter == delimiter => {
                out.push_str(&between);
                between.clear();
                out.push('\n');
                out.push_str(item.indent);
                Marker::Ordered { number: found + 1, width, delimiter }.write_to(&mut out);
                out.push_str(&line[marker_end..]);
                expected += 1;
                through = Some(line_index);
            }
            Some(_) => break,
            None if blank || indent_of(&line).len() > item.indent.len() => {
                between.push('\n');
                between.push_str(&line);
            }
            None => break,
        }
    }
    through.map(|line_index| (out, line_index))
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The parts of `line` if it is a list item by CommonMark's rule.
fn parse_item(line: &str) -> Option<Item<'_>> {
    let indent = indent_of(line);
    let rest = &line[indent.len()..];
    let (marker, marker_len) = marker_at(rest)?;
    let after = &rest[marker_len..];
    let gap_len = after.len() - after.trim_start_matches([' ', '\t']).len();
    // `-5`, `*emphasis*` and `2023.5` are text, not items.
    if gap_len == 0 && !after.is_empty() {
        return None;
    }
    if matches!(marker, Marker::Bullet(_)) && is_thematic_break(line) {
        return None;
    }
    // Five or more spaces start indented code inside the item; the item's own gap is one space.
    let gap = if gap_len > 4 { &after[..1] } else { &after[..gap_len] };
    let marker_end = indent.len() + marker_len;
    let content = &after[gap.len()..];
    let task = !gap.is_empty() && ["[ ]", "[x]", "[X]"].iter().any(|box_| content.starts_with(box_) && content[3..].chars().next().is_none_or(|ch| ch == ' ' || ch == '\t'));
    let text_start = marker_end + gap.len() + if task { 3 } else { 0 };
    Some(Item { indent, marker, marker_end, gap, task, text_start })
}

fn marker_at(rest: &str) -> Option<(Marker, usize)> {
    let first = rest.chars().next()?;
    if matches!(first, '-' | '+' | '*') {
        return Some((Marker::Bullet(first), 1));
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=9).contains(&digits) {
        return None;
    }
    let delimiter = rest[digits..].chars().next().filter(|ch| matches!(ch, '.' | ')'))?;
    let number = rest[..digits].parse().ok()?;
    let width = if digits > 1 && rest.starts_with('0') { digits } else { 0 };
    Some((Marker::Ordered { number, width, delimiter }, digits + 1))
}

/// `- - -`, `* * *` and `***` are rules, not items, even though they start like one.
fn is_thematic_break(line: &str) -> bool {
    let mut marks = line.chars().filter(|ch| !ch.is_whitespace());
    let Some(first) = marks.next() else { return false };
    matches!(first, '-' | '*' | '_') && marks.clone().all(|ch| ch == first) && marks.count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text after Enter at `|` in `text`, with the caret shown as `|`; `None` for a plain new line.
    fn enter(text: &str) -> Option<String> {
        let caret = text.find('|').unwrap();
        let buffer = Buffer::new(&text.replacen('|', "", 1));
        let edit = on_enter(&buffer, caret)?;
        let mut result = buffer.text();
        result.replace_range(edit.range, &edit.text);
        result.insert(edit.caret, '|');
        Some(result)
    }

    #[test]
    fn enter_after_a_numbered_item_starts_the_next_number() {
        assert_eq!(enter("1. Apple\n2. Oranges|").as_deref(), Some("1. Apple\n2. Oranges\n3. |"));
        assert_eq!(enter("1) one|").as_deref(), Some("1) one\n2) |"));
        assert_eq!(enter("9. nine|").as_deref(), Some("9. nine\n10. |"));
        assert_eq!(enter("09. nine|").as_deref(), Some("09. nine\n10. |"));
        assert_eq!(enter("01. one|").as_deref(), Some("01. one\n02. |"));
    }

    #[test]
    fn bullets_tasks_and_nested_items_keep_their_marker_and_indent() {
        assert_eq!(enter("- a|").as_deref(), Some("- a\n- |"));
        assert_eq!(enter("* a|").as_deref(), Some("* a\n* |"));
        assert_eq!(enter("  + nested|").as_deref(), Some("  + nested\n  + |"));
        assert_eq!(enter("- [x] done|").as_deref(), Some("- [x] done\n- [ ] |"));
        assert_eq!(enter("1.  wide gap|").as_deref(), Some("1.  wide gap\n2.  |"));
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        assert_eq!(enter("1. Apple\n2. |").as_deref(), Some("1. Apple\n|"));
        assert_eq!(enter("- a\n- [ ] |").as_deref(), Some("- a\n|"));
        assert_eq!(enter("- a\n-|").as_deref(), Some("- a\n|"));
    }

    #[test]
    fn enter_inside_an_item_moves_the_rest_into_the_new_one() {
        assert_eq!(enter("1. Apple| pie").as_deref(), Some("1. Apple\n2. |pie"));
    }

    #[test]
    fn later_items_are_renumbered_but_deliberate_numbers_are_kept() {
        assert_eq!(enter("1. a|\n2. b\n3. c").as_deref(), Some("1. a\n2. |\n3. b\n4. c"));
        // Nested lines and a single blank line between items stay as they are.
        assert_eq!(enter("1. a|\n   more\n\n2. b\n   - sub\n3. c\n").as_deref(), Some("1. a\n2. |\n   more\n\n3. b\n   - sub\n4. c\n"));
        // A number that does not follow on, a different delimiter or a paragraph ends the renumbering.
        assert_eq!(enter("1. a|\n2. b\n7. g").as_deref(), Some("1. a\n2. |\n3. b\n7. g"));
        assert_eq!(enter("1. a|\n2) b").as_deref(), Some("1. a\n2. |\n2) b"));
        assert_eq!(enter("1. a|\nText\n2. b").as_deref(), Some("1. a\n2. |\nText\n2. b"));
    }

    #[test]
    fn text_that_only_looks_like_a_list_breaks_the_line_plainly() {
        for text in ["-5 degrees|", "*emphasis* here|", "2023 was a year|", "2023.5 units|", "- - -|", "* * *|", "***|", "plain|", "|- a", "1|. a"] {
            assert_eq!(enter(text), None, "{text}");
        }
    }
}
