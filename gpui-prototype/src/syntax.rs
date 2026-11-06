//! Syntax colours for both panes, in the app's palette (`--hl-*` in styles.css).
//!
//! Code in fences is highlighted with tree-sitter, as Zed does. The editor's Markdown syntax
//! comes from the same pulldown-cmark parse the preview uses, so the two panes agree on what is
//! markup. Everything here runs on the background parse thread.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{FontStyle, FontWeight, HighlightStyle, StrikethroughStyle, px, rgb};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Heading,
    Link,
    Code,
    Quote,
    Muted,
    Strong,
    Emphasis,
    Strike,
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Property,
}

impl Token {
    /// The app's editor style for each tag (editor.ts) and its `--hl-*` light-dark pairs.
    pub fn style(self, dark: bool) -> HighlightStyle {
        let color = |light: u32, dark_value: u32| HighlightStyle { color: Some(rgb(if dark { dark_value } else { light }).into()), ..HighlightStyle::default() };
        match self {
            Token::Heading => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..color(0x0550ae, 0x79c0ff) },
            Token::Link => color(0x0969da, 0x58a6ff),
            Token::Code => color(0x953800, 0xffa657),
            Token::Quote => color(0x59636e, 0x9da5b0),
            Token::Muted => color(0x636c76, 0x9198a1),
            Token::Strong => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..HighlightStyle::default() },
            Token::Emphasis => HighlightStyle { font_style: Some(FontStyle::Italic), ..HighlightStyle::default() },
            Token::Strike => HighlightStyle { strikethrough: Some(StrikethroughStyle { thickness: px(1.), color: None }), ..HighlightStyle::default() },
            Token::Keyword => color(0xcf222e, 0xff7b72),
            Token::String => color(0x0a3069, 0xa5d6ff),
            Token::Number => color(0x0550ae, 0x79c0ff),
            Token::Comment => HighlightStyle { font_style: Some(FontStyle::Italic), ..color(0x59636e, 0x9198a1) },
            Token::Function => color(0x8250df, 0xd2a8ff),
            Token::Type => color(0x953800, 0xffa657),
            Token::Property => color(0x0550ae, 0x79c0ff),
        }
    }
}

/// Byte range within a line (editor) or a code block (preview), and its token.
pub type Span = (Range<usize>, Token);

/// Capture names the grammars' highlight queries use, and the app colour each one gets.
/// tree-sitter matches the longest listed prefix, so `function.method` uses `function`.
const CAPTURES: [(&str, Option<Token>); 23] = [
    ("keyword", Some(Token::Keyword)),
    ("operator", None),
    // JSON keys: the app colours them as properties (Lezer's propertyName).
    ("string.special.key", Some(Token::Property)),
    ("string", Some(Token::String)),
    ("escape", Some(Token::String)),
    ("number", Some(Token::Number)),
    ("boolean", Some(Token::Number)),
    ("constant.builtin", Some(Token::Number)),
    ("constant", None),
    ("comment", Some(Token::Comment)),
    ("function", Some(Token::Function)),
    ("constructor", Some(Token::Type)),
    ("type", Some(Token::Type)),
    ("module", Some(Token::Type)),
    ("tag", Some(Token::Type)),
    ("property", Some(Token::Property)),
    ("attribute", Some(Token::Property)),
    ("label", Some(Token::Property)),
    ("punctuation", Some(Token::Muted)),
    ("variable.builtin", Some(Token::Keyword)),
    ("variable", None),
    ("embedded", None),
    ("text.title", Some(Token::Heading)),
];

/// The grammars in this build: one Cargo feature each (see Cargo.toml for their sizes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Lang {
    #[cfg(feature = "lang-bash")]
    Bash,
    #[cfg(feature = "lang-c")]
    C,
    #[cfg(feature = "lang-css")]
    Css,
    #[cfg(feature = "lang-go")]
    Go,
    #[cfg(feature = "lang-html")]
    Html,
    #[cfg(feature = "lang-javascript")]
    JavaScript,
    #[cfg(feature = "lang-json")]
    Json,
    #[cfg(feature = "lang-python")]
    Python,
    #[cfg(feature = "lang-rust")]
    Rust,
    #[cfg(feature = "lang-toml")]
    Toml,
    #[cfg(feature = "lang-typescript")]
    TypeScript,
    #[cfg(feature = "lang-typescript")]
    Tsx,
    #[cfg(feature = "lang-yaml")]
    Yaml,
}

/// Every grammar compiled into this build.
#[cfg(test)]
// Each push is behind its language's feature, which `vec![]` cannot express.
#[allow(clippy::vec_init_then_push)]
fn all_langs() -> Vec<Lang> {
    let mut langs = Vec::new();
    #[cfg(feature = "lang-bash")]
    langs.push(Lang::Bash);
    #[cfg(feature = "lang-c")]
    langs.push(Lang::C);
    #[cfg(feature = "lang-css")]
    langs.push(Lang::Css);
    #[cfg(feature = "lang-go")]
    langs.push(Lang::Go);
    #[cfg(feature = "lang-html")]
    langs.push(Lang::Html);
    #[cfg(feature = "lang-javascript")]
    langs.push(Lang::JavaScript);
    #[cfg(feature = "lang-json")]
    langs.push(Lang::Json);
    #[cfg(feature = "lang-python")]
    langs.push(Lang::Python);
    #[cfg(feature = "lang-rust")]
    langs.push(Lang::Rust);
    #[cfg(feature = "lang-toml")]
    langs.push(Lang::Toml);
    #[cfg(feature = "lang-typescript")]
    langs.extend([Lang::TypeScript, Lang::Tsx]);
    #[cfg(feature = "lang-yaml")]
    langs.push(Lang::Yaml);
    langs
}

/// The fence's info string, or an injection's language name, as a grammar in this build.
/// Unknown names, and languages left out of the build, stay plain.
fn lang_of(name: &str) -> Option<Lang> {
    let name = name.trim().split(|c: char| c.is_whitespace() || c == ',' || c == '{').next()?.to_ascii_lowercase();
    Some(match name.as_str() {
        #[cfg(feature = "lang-bash")]
        "bash" | "sh" | "shell" | "zsh" | "console" => Lang::Bash,
        #[cfg(feature = "lang-c")]
        "c" | "h" => Lang::C,
        #[cfg(feature = "lang-css")]
        "css" => Lang::Css,
        #[cfg(feature = "lang-go")]
        "go" | "golang" => Lang::Go,
        #[cfg(feature = "lang-html")]
        "html" | "htm" | "xhtml" => Lang::Html,
        #[cfg(feature = "lang-javascript")]
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "node" => Lang::JavaScript,
        #[cfg(feature = "lang-json")]
        "json" | "jsonc" | "json5" => Lang::Json,
        #[cfg(feature = "lang-python")]
        "py" | "python" | "python3" => Lang::Python,
        #[cfg(feature = "lang-rust")]
        "rs" | "rust" => Lang::Rust,
        #[cfg(feature = "lang-toml")]
        "toml" => Lang::Toml,
        #[cfg(feature = "lang-typescript")]
        "ts" | "typescript" | "mts" | "cts" => Lang::TypeScript,
        #[cfg(feature = "lang-typescript")]
        "tsx" => Lang::Tsx,
        // Without its own grammar, TypeScript is JavaScript plus type annotations; the JavaScript
        // grammar still colours its keywords, strings, comments and calls.
        #[cfg(all(feature = "lang-javascript", not(feature = "lang-typescript")))]
        "ts" | "typescript" | "mts" | "cts" | "tsx" => Lang::JavaScript,
        #[cfg(feature = "lang-yaml")]
        "yaml" | "yml" => Lang::Yaml,
        _ => return None,
    })
}

/// In tree-sitter-highlight 0.27, when several patterns capture the same node the **later** one wins.
/// So queries are joined general first, specific last: JavaScript, then TypeScript, then JSX.
fn build(lang: Lang) -> Option<HighlightConfiguration> {
    let (language, highlights, injections): (Language, String, &str) = match lang {
        #[cfg(feature = "lang-bash")]
        Lang::Bash => (tree_sitter_bash::LANGUAGE.into(), tree_sitter_bash::HIGHLIGHT_QUERY.into(), ""),
        #[cfg(feature = "lang-c")]
        Lang::C => (tree_sitter_c::LANGUAGE.into(), tree_sitter_c::HIGHLIGHT_QUERY.into(), ""),
        #[cfg(feature = "lang-css")]
        Lang::Css => (tree_sitter_css::LANGUAGE.into(), tree_sitter_css::HIGHLIGHTS_QUERY.into(), ""),
        #[cfg(feature = "lang-go")]
        Lang::Go => (tree_sitter_go::LANGUAGE.into(), tree_sitter_go::HIGHLIGHTS_QUERY.into(), ""),
        // HTML injects CSS and JavaScript; run_highlighter resolves them when those grammars are built in.
        #[cfg(feature = "lang-html")]
        Lang::Html => (tree_sitter_html::LANGUAGE.into(), tree_sitter_html::HIGHLIGHTS_QUERY.into(), tree_sitter_html::INJECTIONS_QUERY),
        #[cfg(feature = "lang-javascript")]
        Lang::JavaScript => (tree_sitter_javascript::LANGUAGE.into(), format!("{}\n{}", tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY), ""),
        // tree-sitter-json 0.24's query puts the key pattern before `(string) @string`, written for the
        // older first-wins rule; repeating it at the end gives keys the property colour, as in the app.
        #[cfg(feature = "lang-json")]
        Lang::Json => (tree_sitter_json::LANGUAGE.into(), format!("{}\n(pair key: (_) @string.special.key)", tree_sitter_json::HIGHLIGHTS_QUERY), ""),
        #[cfg(feature = "lang-python")]
        Lang::Python => (tree_sitter_python::LANGUAGE.into(), tree_sitter_python::HIGHLIGHTS_QUERY.into(), ""),
        #[cfg(feature = "lang-rust")]
        Lang::Rust => (tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY.into(), ""),
        #[cfg(feature = "lang-toml")]
        Lang::Toml => (tree_sitter_toml_ng::LANGUAGE.into(), tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(), ""),
        #[cfg(feature = "lang-typescript")]
        Lang::TypeScript => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            format!("{}\n{}", tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_typescript::HIGHLIGHTS_QUERY),
            "",
        ),
        #[cfg(feature = "lang-typescript")]
        Lang::Tsx => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            format!(
                "{}\n{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            ),
            "",
        ),
        #[cfg(feature = "lang-yaml")]
        Lang::Yaml => (tree_sitter_yaml::LANGUAGE.into(), tree_sitter_yaml::HIGHLIGHTS_QUERY.into(), ""),
    };
    let mut config = match HighlightConfiguration::new(language, "", &highlights, injections, "") {
        Ok(config) => config,
        Err(err) => {
            eprintln!("highlight query failed to compile: {err}");
            return None;
        }
    };
    config.configure(&CAPTURES.map(|(name, _)| name));
    Some(config)
}

/// Compiling a query takes milliseconds, so each language is built once, the first time a fence uses it.
fn config(lang: Lang) -> Option<&'static HighlightConfiguration> {
    static CONFIGS: OnceLock<Mutex<HashMap<Lang, Option<&'static HighlightConfiguration>>>> = OnceLock::new();
    let mut configs = CONFIGS.get_or_init(Default::default).lock().ok()?;
    *configs.entry(lang).or_insert_with(|| build(lang).map(|config| &*Box::leak(Box::new(config))))
}

/// Highlights `code` written in `info` (a fence's info string). Results are cached by language and
/// text, so a re-parse after a keystroke only highlights the code block that changed.
pub fn highlight_code(info: &str, code: &str) -> Option<Arc<Vec<Span>>> {
    let lang = lang_of(info)?;
    let key = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (lang, code).hash(&mut hasher);
        hasher.finish()
    };
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Vec<Span>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(spans) = cache.lock().ok()?.get(&key) {
        return Some(spans.clone());
    }
    let spans = Arc::new(run_highlighter(config(lang)?, code));
    let mut cache = cache.lock().ok()?;
    // A cheap bound: forget everything when it grows past what a few large documents need.
    if cache.len() > 20_000 {
        cache.clear();
    }
    cache.insert(key, spans.clone());
    Some(spans)
}

fn run_highlighter(root: &'static HighlightConfiguration, code: &str) -> Vec<Span> {
    let mut highlighter = Highlighter::new();
    // Injections, such as CSS and JavaScript inside HTML, use their own language's configuration.
    let Ok(events) = highlighter.highlight(root, code.as_bytes(), None, None, |name| lang_of(name).and_then(config)) else {
        return Vec::new();
    };
    let mut spans: Vec<Span> = Vec::new();
    let mut stack: Vec<Option<Token>> = Vec::new();
    for event in events {
        match event {
            Ok(HighlightEvent::HighlightStart(highlight)) => stack.push(CAPTURES.get(highlight.0).and_then(|(_, token)| *token)),
            Ok(HighlightEvent::HighlightEnd) => {
                stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                let Some(token) = stack.iter().rev().find_map(|token| *token) else { continue };
                match spans.last_mut() {
                    Some((range, last)) if *last == token && range.end == start => range.end = end,
                    _ => spans.push((start..end, token)),
                }
            }
            Err(_) => break,
        }
    }
    spans
}

/// The editor's colours for the whole document, per line: Markdown syntax from pulldown-cmark and
/// code in fences from tree-sitter. Offsets are relative to each line's start, so an edit only
/// invalidates the lines it touches until the next parse.
pub fn source_spans(text: &str) -> Vec<Vec<Span>> {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(index, _)| index + 1)).collect();
    let mut lines: Vec<Vec<Span>> = vec![Vec::new(); line_starts.len()];
    let mut add = |range: Range<usize>, token: Token| {
        if range.is_empty() {
            return;
        }
        // Split at line ends; a span never crosses into the next line.
        let mut line = line_starts.partition_point(|start| *start <= range.start) - 1;
        let mut start = range.start;
        while start < range.end && line < line_starts.len() {
            let line_end = line_starts.get(line + 1).map(|next| next - 1).unwrap_or(text.len());
            let end = range.end.min(line_end);
            if end > start {
                lines[line].push((start - line_starts[line]..end - line_starts[line], token));
            }
            line += 1;
            start = line_starts.get(line).copied().unwrap_or(text.len());
        }
    };

    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut fence: Option<(String, Range<usize>, Option<Range<usize>>)> = None;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => add(range, Token::Heading),
            Event::Start(Tag::Emphasis) => add(range, Token::Emphasis),
            Event::Start(Tag::Strong) => add(range, Token::Strong),
            Event::Start(Tag::Strikethrough) => add(range, Token::Strike),
            Event::Start(Tag::Link { .. }) | Event::Start(Tag::Image { .. }) => add(range, Token::Link),
            Event::Start(Tag::BlockQuote(_)) => add(range, Token::Quote),
            Event::Start(Tag::MetadataBlock(_)) | Event::Rule => add(range, Token::Muted),
            Event::Code(_) => add(range, Token::Code),
            Event::Start(Tag::Item) => {
                let marker = text[range.start..].bytes().take_while(|b| b.is_ascii_digit() || matches!(b, b'-' | b'*' | b'+' | b'.' | b')')).count();
                add(range.start..range.start + marker, Token::Muted);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                fence = Some((info, range, None));
            }
            Event::Text(_) if fence.is_some() => {
                if let Some((_, _, content)) = fence.as_mut() {
                    *content = Some(match content.take() {
                        Some(content) => content.start..range.end,
                        None => range,
                    });
                }
            }
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                let Some((info, block, content)) = fence.take() else { continue };
                let content = content.unwrap_or(block.end..block.end);
                // The fence lines are markup; the code between them gets its language's colours.
                add(block.start..content.start, Token::Muted);
                add(content.end..block.end, Token::Muted);
                let code = text[content.clone()].trim_end_matches('\n');
                if let Some(spans) = highlight_code(&info, code) {
                    for (span, token) in spans.iter() {
                        add(content.start + span.start..content.start + span.end, *token);
                    }
                }
            }
            _ => {}
        }
    }
    for line in &mut lines {
        line.sort_by_key(|(range, _)| range.start);
    }
    lines
}

/// Folds overlapping styles in order, later ones on top, into the non-overlapping runs a
/// `StyledText` needs. GPUI's `combine_highlights` folds in hash-set order, so overlapping colours
/// (a link inside a heading) could come out either way.
pub fn merge(spans: &[(Range<usize>, HighlightStyle)]) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut points: Vec<usize> = spans.iter().flat_map(|(range, _)| [range.start, range.end]).collect();
    points.sort_unstable();
    points.dedup();
    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for pair in points.windows(2) {
        let segment = pair[0]..pair[1];
        let mut covering = spans.iter().filter(|(range, _)| range.start <= segment.start && segment.end <= range.end).peekable();
        if covering.peek().is_none() {
            continue;
        }
        let style = covering.fold(HighlightStyle::default(), |style, (_, next)| style.highlight(*next));
        match merged.last_mut() {
            Some((range, last)) if *last == style && range.end == segment.start => range.end = segment.end,
            _ => merged.push((segment, style)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grammar_and_its_query_compile() {
        for lang in all_langs() {
            assert!(config(lang).is_some(), "{lang:?}");
        }
    }

    fn token_at(code: &str, spans: &[Span], needle: &str) -> Option<Token> {
        let at = code.find(needle).unwrap();
        spans.iter().find(|(range, _)| range.start <= at && at < range.end).map(|(_, token)| *token)
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn javascript_keywords_calls_strings_and_comments_get_their_colours() {
        let code = "function main() {\n  const s = \"hi\"; // greet\n  return s;\n}";
        let spans = highlight_code("js", code).unwrap();
        assert_eq!(token_at(code, &spans, "function"), Some(Token::Keyword));
        assert_eq!(token_at(code, &spans, "main"), Some(Token::Function));
        assert_eq!(token_at(code, &spans, "\"hi\""), Some(Token::String));
        assert_eq!(token_at(code, &spans, "// greet"), Some(Token::Comment));
        assert!(highlight_code("not-a-language", code).is_none());
    }

    #[test]
    #[cfg(all(feature = "lang-javascript", not(feature = "lang-typescript")))]
    fn typescript_fences_fall_back_to_the_javascript_grammar() {
        let code = "const total: number = 1; // sum";
        let spans = highlight_code("ts", code).unwrap();
        assert_eq!(token_at(code, &spans, "const"), Some(Token::Keyword));
        assert_eq!(token_at(code, &spans, "// sum"), Some(Token::Comment));
    }

    #[test]
    #[cfg(feature = "lang-python")]
    fn python_definitions_and_strings_get_their_colours() {
        let code = "def greet(name):\n    return f\"hi {name}\"  # hello";
        let spans = highlight_code("python", code).unwrap();
        assert_eq!(token_at(code, &spans, "def"), Some(Token::Keyword));
        assert_eq!(token_at(code, &spans, "greet"), Some(Token::Function));
        assert_eq!(token_at(code, &spans, "# hello"), Some(Token::Comment));
    }

    #[test]
    fn markdown_spans_are_split_per_line_and_fences_colour_their_code() {
        let text = "# Title\n\nSome **bold** and `code`.\n\n```json\n{\"a\": 1}\n```\n";
        let lines = source_spans(text);
        assert_eq!(lines[0], vec![(0..7, Token::Heading)]);
        assert!(lines[2].contains(&(5..13, Token::Strong)));
        assert!(lines[2].contains(&(18..24, Token::Code)));
        assert_eq!(lines[4], vec![(0..7, Token::Muted)]);
        assert!(lines[5].iter().any(|(_, token)| *token == Token::Property));
        assert!(lines[5].iter().any(|(_, token)| *token == Token::Number));
        assert_eq!(lines[6], vec![(0..3, Token::Muted)]);
    }

    #[test]
    fn merge_layers_styles_in_order() {
        let bold = Token::Strong.style(true);
        let red = HighlightStyle { color: Some(rgb(0xff0000).into()), ..HighlightStyle::default() };
        let merged = merge(&[(0..10, bold), (2..4, red)]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].0, 2..4);
        assert_eq!(merged[1].1.font_weight, Some(FontWeight::BOLD));
        assert_eq!(merged[1].1.color, red.color);
    }
}
