//! WCAG 2.2 contrast for Marky's colours, in both themes: 4.5:1 for text (AA), 3:1 for the focus
//! ring against what it is drawn on.
//! Save's dimmed label is left out: WCAG exempts inactive controls.

#![cfg(test)]

use gpui::{Rgba, rgb};

use crate::syntax::Token;

fn luminance(color: Rgba) -> f32 {
    let channel = |value: f32| if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn ratio(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// (what, text, background) in one theme, as `0xRRGGBB` values.
fn text_pairs(dark: bool) -> Vec<(&'static str, u32, u32)> {
    let pick = |dark_value: u32, light_value: u32| if dark { dark_value } else { light_value };
    let text = pick(0xfafafa, 0x18181b);
    let quiet = pick(0xa1a1aa, 0x52525b);
    vec![
        ("text on the editor", text, pick(0x18181b, 0xfafafa)),
        ("text on the preview", text, pick(0x09090b, 0xffffff)),
        ("text on the toolbar and tabs", text, pick(0x27272a, 0xe4e4e7)),
        ("text on the menu bar", text, pick(0x27272a, 0xf4f4f5)),
        ("text on a menu", text, pick(0x3f3f46, 0xffffff)),
        ("text on a highlighted menu item", text, pick(0x52525b, 0xdbeafe)),
        ("menu shortcut hints", pick(0xd4d4d8, 0x52525b), pick(0x3f3f46, 0xffffff)),
        ("shortcut hints on a highlighted item", pick(0xd4d4d8, 0x52525b), pick(0x52525b, 0xdbeafe)),
        ("quiet text on the path and status bars", quiet, pick(0x27272a, 0xe4e4e7)),
        ("quiet text on the outline and settings", quiet, pick(0x18181b, 0xf4f4f5)),
        ("quiet text on the preview", quiet, pick(0x09090b, 0xffffff)),
        ("the current heading in the outline", pick(0x60a5fa, 0x2563eb), pick(0x18181b, 0xf4f4f5)),
        ("links in the preview", pick(0x58a6ff, 0x0969da), pick(0x09090b, 0xffffff)),
        ("errors on the path bar", pick(0xfca5a5, 0xb91c1c), pick(0x27272a, 0xe4e4e7)),
        ("errors in the preview", pick(0xfca5a5, 0xb91c1c), pick(0x09090b, 0xffffff)),
        ("text on a selection", text, pick(0x1e3a8a, 0xbfdbfe)),
        ("text on a find match", text, pick(0x713f12, 0xfef08a)),
        ("text on ==marked== text", text, pick(0x4b3218, 0xffeed8)),
        ("text on inline code", text, pick(0x3f3f46, 0xe4e4e7)),
    ]
}

#[test]
fn text_meets_aa_in_both_themes() {
    let mut failures = Vec::new();
    for dark in [false, true] {
        for (what, text, background) in text_pairs(dark) {
            let contrast = ratio(rgb(text), rgb(background));
            if contrast < 4.5 {
                failures.push(format!("{} theme, {what}: {contrast:.2}:1", if dark { "dark" } else { "light" }));
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1:\n{}", failures.join("\n"));
}

#[test]
fn syntax_colours_meet_aa_on_the_editor() {
    let tokens = [
        Token::Heading,
        Token::Link,
        Token::Code,
        Token::Quote,
        Token::Muted,
        Token::Strong,
        Token::Emphasis,
        Token::Strike,
        Token::Keyword,
        Token::String,
        Token::Number,
        Token::Comment,
        Token::Function,
        Token::Type,
        Token::Property,
    ];
    let mut failures = Vec::new();
    for dark in [false, true] {
        let background = rgb(if dark { 0x18181b } else { 0xfafafa });
        for token in tokens {
            let Some(color) = token.style(dark).color else { continue };
            let contrast = ratio(Rgba::from(color), background);
            if contrast < 4.5 {
                failures.push(format!("{} theme, {token:?}: {contrast:.2}:1", if dark { "dark" } else { "light" }));
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1:\n{}", failures.join("\n"));
}

#[test]
fn the_focus_ring_stands_out_from_every_background_it_is_drawn_on() {
    for dark in [false, true] {
        let ring = crate::editor::ring(dark);
        let backgrounds: &[u32] = if dark { &[0x27272a, 0x18181b, 0x09090b, 0x3f3f46] } else { &[0xe4e4e7, 0xf4f4f5, 0xffffff, 0xfafafa] };
        for background in backgrounds {
            let contrast = ratio(ring, rgb(*background));
            assert!(contrast >= 3.0, "ring on {background:06x}: {contrast:.2}:1");
        }
    }
}
