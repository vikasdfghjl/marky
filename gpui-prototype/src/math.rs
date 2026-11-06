//! TeX formulas for the preview, laid out with RaTeX (KaTeX-compatible, pure Rust, KaTeX's fonts
//! embedded) and drawn by GPUI as SVG. No JavaScript, as the migration doc requires; the reasons and
//! sizes are in DECISION.md.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{Image, ImageFormat};
use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::color::Color;
use ratex_types::math_style::MathStyle;

/// The app's `.katex { font-size: 1.21em }`: formulas are a little larger than the text around them.
const KATEX_SCALE: f32 = 1.21;

/// A formula as an SVG image and its size in pixels.
pub struct Formula {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
}

type Rendered = Result<Arc<Formula>, Arc<str>>;

/// TeX, display style, font size bits and dark theme: everything the layout depends on.
type Key = (String, bool, u32, bool);

/// Lays out `tex` once per size, colour and style; later frames reuse the image. Only formulas the
/// preview actually draws are laid out, because the preview is a virtualised list.
pub fn render(tex: &str, display: bool, font_size: f32, dark: bool) -> Rendered {
    let key: Key = (tex.to_string(), display, font_size.to_bits(), dark);
    static CACHE: OnceLock<Mutex<HashMap<Key, Rendered>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return found;
    }
    let rendered = lay_out(tex, display, font_size * KATEX_SCALE, dark);
    if let Ok(mut cache) = cache.lock() {
        // A cheap bound: forget everything when it grows past what a few large documents need.
        if cache.len() > 4_000 {
            cache.clear();
        }
        cache.insert(key, rendered.clone());
    }
    rendered
}

fn lay_out(tex: &str, display: bool, em: f32, dark: bool) -> Rendered {
    let ast = ratex_parser::parser::parse(tex).map_err(|err| Arc::from(err.to_string()))?;
    // The preview's text colour (styles.css --fg), so formulas follow the theme.
    let color = if dark { Color::rgb(0.98, 0.98, 0.98) } else { Color::rgb(0.094, 0.094, 0.106) };
    let options = LayoutOptions { style: if display { MathStyle::Display } else { MathStyle::Text }, color, ..LayoutOptions::default() };
    let boxed = layout(&ast, &options);
    validate_layout(&boxed).map_err(Arc::from)?;
    let svg = render_to_svg(&to_display_list(&boxed), &SvgOptions { font_size: em as f64, padding: 1.0, embed_glyphs: true, ..SvgOptions::default() });
    let (width, height) = view_box_size(&svg).ok_or_else(|| Arc::from("RaTeX returned an SVG without a size"))?;
    Ok(Arc::new(Formula { image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())), width, height }))
}

/// Width and height from the root `viewBox`, whose units are pixels at the requested font size.
/// The `width` and `height` attributes are in points, which would draw a third too large.
fn view_box_size(svg: &str) -> Option<(f32, f32)> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let values: Vec<f32> = svg[start..].split('"').next()?.split_whitespace().filter_map(|value| value.parse().ok()).collect();
    match values.as_slice() {
        [_, _, width, height] => Some((*width, *height)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_render_and_broken_tex_reports_an_error() {
        let fraction = render(r"\frac{n(n+1)}{2}", true, 16.0, true).unwrap();
        let inline = render(r"\frac{n(n+1)}{2}", false, 16.0, true).unwrap();
        assert!(fraction.width > 0.0 && fraction.height > inline.height);
        assert!(render(r"\frac{1}{", false, 16.0, true).is_err());
    }

    #[test]
    fn a_formula_is_laid_out_once_per_size_and_theme() {
        let first = render("x^2", false, 15.0, false).unwrap();
        let again = render("x^2", false, 15.0, false).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &render("x^2", false, 15.0, true).unwrap()));
    }
}
