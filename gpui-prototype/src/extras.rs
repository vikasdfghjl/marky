//! Inline rules the app gets from markdown-it and its plugins that pulldown-cmark does not have:
//! abbreviations (`*[HTML]: …`, markdown-it-abbr), bare links (`linkify`, with linkify-it's
//! defaults), the typographer's replacements beyond quotes and dashes, and `$…$` that the app's
//! math rule accepts but pulldown-cmark leaves as text (it wants balanced braces).
//!
//! Each works on a block's runs after pulldown-cmark has parsed it, and leaves code, formulas and
//! (except abbreviations) link text alone.

use std::sync::Arc;

use crate::preview::{Run, RunStyle};

// ----- Abbreviations -----

/// `*[label]: title` definitions, longest label first so `HTML5` wins over `HTML`.
#[derive(Clone, Debug, Default)]
pub struct Abbreviations(Vec<(String, Arc<str>)>);

impl Abbreviations {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One definition line: `*[label]: title`, up to three spaces in, as markdown-it-abbr reads it.
pub fn abbreviation_definition(line: &str) -> Option<(&str, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line.trim_start_matches(' ').strip_prefix("*[")?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() || label.contains(['[', ']']) {
        return None;
    }
    Some((label, rest[close + 2..].trim()))
}

/// The definitions in a document, skipping fenced code. A later definition of a label is ignored,
/// as in markdown-it-abbr.
pub fn collect_abbreviations(markdown: &str) -> Abbreviations {
    let mut found: Vec<(String, Arc<str>)> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|marker| trimmed.starts_with(marker));
        match (fence, marker) {
            (Some(open), Some(marker)) if marker == open => fence = None,
            (None, Some(marker)) => fence = Some(marker),
            _ => {}
        }
        if fence.is_some() {
            continue;
        }
        if let Some((label, title)) = abbreviation_definition(line) {
            if !found.iter().any(|(known, _)| known == label) {
                found.push((label.to_string(), Arc::from(title)));
            }
        }
    }
    found.sort_by_key(|(label, _)| std::cmp::Reverse(label.len()));
    Abbreviations(found)
}

/// A paragraph made only of definitions is not shown, as markdown-it-abbr removes them.
pub fn only_definitions(source: &str) -> bool {
    let mut lines = source.lines().filter(|line| !line.trim().is_empty()).peekable();
    lines.peek().is_some() && lines.all(|line| abbreviation_definition(line).is_some())
}

/// markdown-it-abbr's boundary: start or end of text, white space or punctuation.
fn abbreviation_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| ch.is_whitespace() || ch.is_ascii_punctuation() || (!ch.is_ascii() && !ch.is_alphanumeric()))
}

fn split_abbreviations(text: &str, style: &RunStyle, abbreviations: &Abbreviations, out: &mut Vec<Run>) {
    let mut rest = text;
    'scan: while !rest.is_empty() {
        let mut best: Option<(usize, &str, &Arc<str>)> = None;
        for (label, title) in &abbreviations.0 {
            let mut from = 0;
            while let Some(found) = rest[from..].find(label.as_str()) {
                let at = from + found;
                let before = rest[..at].chars().next_back();
                let after = rest[at + label.len()..].chars().next();
                if abbreviation_boundary(before) && abbreviation_boundary(after) {
                    if best.is_none_or(|(known, _, _)| at < known) {
                        best = Some((at, label, title));
                    }
                    break;
                }
                from = at + label.chars().next().map_or(1, char::len_utf8);
            }
        }
        let Some((at, label, title)) = best else { break 'scan };
        push(out, &rest[..at], style.clone());
        push(out, label, RunStyle { abbr: Some(title.clone()), ..style.clone() });
        rest = &rest[at + label.len()..];
    }
    push(out, rest, style.clone());
}

// ----- Typographer -----

/// markdown-it's `replacements` that pulldown-cmark's smart punctuation lacks: (c), (r), (tm) in
/// any case, and +-. Quotes, dashes and ellipses come from pulldown-cmark itself.
pub fn replacements(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['(', '+']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let lower = tail.get(..4).unwrap_or(tail).to_ascii_lowercase();
        let (replacement, used) = if lower.starts_with("(c)") {
            ("©", 3)
        } else if lower.starts_with("(r)") {
            ("®", 3)
        } else if lower.starts_with("(tm)") {
            ("™", 4)
        } else if tail.starts_with("+-") {
            ("±", 2)
        } else {
            (&tail[..1], 1)
        };
        out.push_str(replacement);
        rest = &tail[used..];
    }
    out.push_str(rest);
    out
}

// ----- Bare links (linkify-it's defaults: fuzzy links and e-mail on, IP addresses off) -----

/// linkify-it's default top-level domains for links without a scheme, besides any two letters.
const TLDS: [&str; 17] = ["biz", "com", "edu", "gov", "net", "org", "pro", "web", "xxx", "aero", "asia", "coop", "info", "museum", "name", "shop", "рф"];

/// The next bare link in `text`: its byte range and its destination.
pub fn next_link(text: &str) -> Option<(std::ops::Range<usize>, String)> {
    let mut from = 0;
    while from < text.len() {
        let (at, _) = text[from..].char_indices().find(|(offset, ch)| ch.is_alphanumeric() && word_start(text, from + offset))?;
        let start = from + at;
        if let Some(found) = link_at(text, start) {
            return Some(found);
        }
        // Skip the rest of this word.
        let word_end = text[start..].find(|ch: char| !(ch.is_alphanumeric() || "-_.@".contains(ch))).map_or(text.len(), |end| start + end);
        from = word_end.max(start + 1);
        while !text.is_char_boundary(from) {
            from += 1;
        }
    }
    None
}

fn word_start(text: &str, at: usize) -> bool {
    text[..at].chars().next_back().is_none_or(|ch| !(ch.is_alphanumeric() || "-_.@/:".contains(ch)))
}

fn link_at(text: &str, start: usize) -> Option<(std::ops::Range<usize>, String)> {
    let tail = &text[start..];
    let lower = tail.get(..8).unwrap_or(tail).to_ascii_lowercase();
    for scheme in ["https://", "http://", "ftp://", "mailto:"] {
        if lower.starts_with(scheme) {
            let end = start + link_end(&tail[scheme.len()..]) + scheme.len();
            if end > start + scheme.len() {
                return Some((start..end, text[start..end].to_string()));
            }
            return None;
        }
    }
    if lower.starts_with("www.") {
        let end = start + link_end(tail);
        let host_end = text[start..end].find(['/', '?', '#', ':']).map_or(end, |offset| start + offset);
        return valid_host(&text[start + 4..host_end], false).then(|| (start..end, format!("http://{}", &text[start..end])));
    }
    // An e-mail address: local part, @, a host with a known top-level domain.
    if let Some(at) = tail.find('@').filter(|at| *at > 0 && tail[..*at].chars().all(|ch| ch.is_alphanumeric() || "._%+-".contains(ch))) {
        let host = &tail[at + 1..];
        let host_len = host.find(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '.')).unwrap_or(host.len());
        let host = host[..host_len].trim_end_matches('.');
        if valid_host(host, true) {
            let end = start + at + 1 + host.len();
            return Some((start..end, format!("mailto:{}", &text[start..end])));
        }
        return None;
    }
    // A host with a known top-level domain, with an optional port and path.
    let host_len = tail.find(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '.')).unwrap_or(tail.len());
    let host = tail[..host_len].trim_end_matches('.');
    if !valid_host(host, true) {
        return None;
    }
    let after_host = &tail[host.len()..];
    let end = if after_host.starts_with(['/', ':', '?', '#']) { start + host.len() + link_end(after_host) } else { start + host.len() };
    Some((start..end, format!("http://{}", &text[start..end])))
}

/// Labels of letters, digits and hyphens, at least two, the last a known top-level domain
/// (`fuzzy`) or any letters (after `www.`).
fn valid_host(host: &str, fuzzy: bool) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-')) {
        return false;
    }
    let tld = labels[labels.len() - 1].to_lowercase();
    if !tld.chars().all(char::is_alphabetic) {
        return false;
    }
    !fuzzy || tld.chars().count() == 2 || TLDS.contains(&tld.as_str())
}

/// Where a link that starts at the beginning of `tail` ends: at white space or `<`, less trailing
/// punctuation and a closing bracket that has no opening one in the link.
fn link_end(tail: &str) -> usize {
    let mut end = tail.find(|ch: char| ch.is_whitespace() || "<>\"`".contains(ch)).unwrap_or(tail.len());
    loop {
        let link = &tail[..end];
        let Some(last) = link.chars().next_back() else { return 0 };
        let unbalanced = match last {
            ')' => link.matches('(').count() < link.matches(')').count(),
            ']' => link.matches('[').count() < link.matches(']').count(),
            _ => false,
        };
        if ".,:;!?'*".contains(last) || unbalanced {
            end -= last.len_utf8();
        } else {
            return end;
        }
    }
}

// ----- Inline math the app accepts -----

/// The app's inline rule (`math.ts`): `$` then a non-space, up to a `$` that follows a non-space and
/// precedes no digit. `$$` never opens inline math. Returns the byte offsets of both dollars.
pub fn next_inline_math(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find('$') {
        let open = from + found;
        from = open + 1;
        let next = text[open + 1..].chars().next();
        if open > 0 && bytes[open - 1] == b'$' || next.is_none_or(|ch| ch == '$' || ch.is_whitespace()) {
            continue;
        }
        let mut search = open + 1;
        while let Some(found) = text[search..].find('$') {
            let close = search + found;
            let before = text[..close].chars().next_back();
            let after = text[close + 1..].chars().next();
            if before.is_some_and(|ch| !ch.is_whitespace() && ch != '\\') && after.is_none_or(|ch| !ch.is_ascii_digit()) {
                return Some((open, close));
            }
            search = close + 1;
        }
        return None;
    }
    None
}

/// A paragraph that is `$$ … $$` and nothing else, which pulldown-cmark did not take as math: the
/// formula, for a display math block.
pub fn display_math(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("$$")?.strip_suffix("$$")?;
    (!inner.trim().is_empty()).then_some(inner.trim())
}

// ----- Applying them to a block -----

/// Inline math the app accepts and pulldown-cmark left as text. Runs first, before `~sub~` and
/// the rest, so a formula's content is never split or linked.
pub fn apply_math(runs: Vec<Run>) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.code || run.style.math || run.style.link.is_some() || !run.text.contains('$') {
            out.push(run);
            continue;
        }
        let mut rest = run.text.as_str();
        while let Some((open, close)) = next_inline_math(rest) {
            push(&mut out, &rest[..open], run.style.clone());
            out.push(Run { text: rest[open + 1..close].to_string(), style: RunStyle { math: true, ..run.style.clone() } });
            rest = &rest[close + 1..];
        }
        push(&mut out, rest, run.style);
    }
    out
}

/// Bare links, the typographer's replacements and abbreviations, after `~sub~`, `^sup^` and
/// `==mark==` (markdown-it's core rules run after its inline ones).
pub fn apply_text(runs: Vec<Run>, abbreviations: &Abbreviations, typographer: bool) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        if run.style.code || run.style.math {
            out.push(run);
            continue;
        }
        plain(&run.text, &run.style, abbreviations, typographer, &mut out);
    }
    out
}

fn plain(text: &str, style: &RunStyle, abbreviations: &Abbreviations, typographer: bool, out: &mut Vec<Run>) {
    let mut rest = text;
    if style.link.is_none() {
        while let Some((range, destination)) = next_link(rest) {
            finish(&rest[..range.start], style, abbreviations, typographer, out);
            let link = RunStyle { link: Some(Arc::from(destination)), ..style.clone() };
            push(out, &rest[range.clone()], link);
            rest = &rest[range.end..];
        }
    }
    finish(rest, style, abbreviations, typographer, out);
}

fn finish(text: &str, style: &RunStyle, abbreviations: &Abbreviations, typographer: bool, out: &mut Vec<Run>) {
    let replaced;
    let text = if typographer {
        replaced = replacements(text);
        replaced.as_str()
    } else {
        text
    };
    if abbreviations.is_empty() {
        push(out, text, style.clone());
    } else {
        split_abbreviations(text, style, abbreviations, out);
    }
}

fn push(out: &mut Vec<Run>, text: &str, style: RunStyle) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => out.push(Run { text: text.to_string(), style }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some((range, destination)) = next_link(rest) {
            found.push((rest[range.clone()].to_string(), destination));
            rest = &rest[range.end..];
        }
        found
    }

    #[test]
    fn bare_links_follow_linkify_its_defaults() {
        assert_eq!(links("See https://example.com/a_(b). Done"), vec![("https://example.com/a_(b)".into(), "https://example.com/a_(b)".into())]);
        assert_eq!(links("(www.example.org/x)"), vec![("www.example.org/x".into(), "http://www.example.org/x".into())]);
        assert_eq!(links("mail me@example.com, now"), vec![("me@example.com".into(), "mailto:me@example.com".into())]);
        // .dev is not among linkify-it's default top-level domains.
        assert_eq!(links("go to marky.dev or example.com/docs!"), vec![("example.com/docs".into(), "http://example.com/docs".into())]);
        // linkify-it takes any two-letter domain, so a file name like README.md is a link in the app too.
        assert_eq!(links("README.md"), vec![("README.md".into(), "http://README.md".into())]);
        assert!(links("version 1.2.3 and e.g. this, foo.bar").is_empty());
        assert!(links("ftp:// alone").is_empty());
    }

    #[test]
    fn abbreviations_match_whole_words() {
        let abbreviations = collect_abbreviations("*[HTML]: Hyper Text Markup Language\n*[W3C]: World Wide Web Consortium\n```\n*[NO]: in code\n```\n");
        assert_eq!(abbreviations.0.len(), 2);
        let runs = apply_text(vec![Run { text: "HTML and XHTML by the W3C.".into(), style: RunStyle::default() }], &abbreviations, false);
        let marked: Vec<&str> = runs.iter().filter(|run| run.style.abbr.is_some()).map(|run| run.text.as_str()).collect();
        assert_eq!(marked, vec!["HTML", "W3C"]);
        assert!(only_definitions("*[HTML]: Hyper Text\n*[W3C]: Web\n"));
        assert!(!only_definitions("*[HTML]: Hyper Text\nand more text\n"));
    }

    #[test]
    fn replacements_cover_what_pulldown_does_not() {
        assert_eq!(replacements("(c) 2026 (TM) (r) +-5 (x) a+b"), "© 2026 ™ ® ±5 (x) a+b");
    }

    #[test]
    fn inline_math_uses_the_apps_rule() {
        assert_eq!(next_inline_math(r"stays readable: $\frac{1}{$."), Some((16, 26)));
        assert_eq!(next_inline_math("prices like $5 and $10"), None);
        assert_eq!(next_inline_math("$$x$$"), None);
        assert_eq!(display_math(r"$$ \frac{1}{ $$"), Some(r"\frac{1}{"));
        assert_eq!(display_math("$$ $$"), None);
    }
}
