//! TeX formulas for the preview, laid out with RaTeX (KaTeX-compatible, pure Rust, KaTeX's fonts
//! embedded) and drawn by GPUI as SVG. No JavaScript, as the migration doc requires; the reasons and
//! sizes are in DECISION.md.
//!
//! The bitmap is made here rather than by GPUI, so [`release_unused`] can drop the bitmaps of
//! formulas that no open document contains any more (see `bitmaps.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use gpui::{App, Image, ImageFormat, RenderImage, SvgRenderer};
use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::color::Color;
use ratex_types::math_style::MathStyle;

use crate::bitmaps;

/// The app's `.katex { font-size: 1.21em }`: formulas are a little larger than the text around them.
const KATEX_SCALE: f32 = 1.21;

/// A formula as a bitmap (at GPUI's 2× SVG scale) and its size in pixels.
pub struct Formula {
    pub image: Arc<RenderImage>,
    pub width: f32,
    pub height: f32,
}

type Rendered = Result<Arc<Formula>, Arc<str>>;

/// TeX, display style, font size bits and dark theme: everything the layout depends on.
type Key = (String, bool, u32, bool);

fn cache() -> &'static Mutex<HashMap<Key, Rendered>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Rendered>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Lays out and draws `tex` once per size, colour and style; later frames reuse the bitmap. Only
/// formulas the preview actually draws are laid out, because the preview is a virtualised list.
pub fn render(tex: &str, display: bool, font_size: f32, dark: bool, renderer: &SvgRenderer) -> Rendered {
    let key: Key = (tex.to_string(), display, font_size.to_bits(), dark);
    if let Some(found) = cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return found.clone();
    }
    let rendered = lay_out(tex, display, font_size * KATEX_SCALE, dark, renderer);
    cache().lock().unwrap_or_else(PoisonError::into_inner).insert(key, rendered.clone());
    rendered
}

/// Forgets every formula that is not in `in_use` (TeX and display style) at this font size and
/// theme, and drops its bitmap. Call it after the open documents or the view settings change.
pub fn release_unused(in_use: &HashSet<(&str, bool)>, font_size: f32, dark: bool, cx: &mut App) {
    let mut released = Vec::new();
    cache().lock().unwrap_or_else(PoisonError::into_inner).retain(|(tex, display, size, theme), rendered| {
        let keep = *size == font_size.to_bits() && *theme == dark && in_use.contains(&(tex.as_str(), *display));
        if let (false, Ok(formula)) = (keep, rendered) {
            released.push(formula.image.clone());
        }
        keep
    });
    bitmaps::drop_bitmaps(released, cx);
}

fn lay_out(tex: &str, display: bool, em: f32, dark: bool, renderer: &SvgRenderer) -> Rendered {
    let ast = ratex_parser::parser::parse(tex).map_err(|err| Arc::from(err.to_string()))?;
    // The preview's text colour (styles.css --fg), so formulas follow the theme.
    let color = if dark { Color::rgb(0.98, 0.98, 0.98) } else { Color::rgb(0.094, 0.094, 0.106) };
    let options = LayoutOptions { style: if display { MathStyle::Display } else { MathStyle::Text }, color, ..LayoutOptions::default() };
    let boxed = layout(&ast, &options);
    validate_layout(&boxed).map_err(Arc::from)?;
    let svg = render_to_svg(&to_display_list(&boxed), &SvgOptions { font_size: em as f64, padding: 1.0, embed_glyphs: true, ..SvgOptions::default() });
    let (width, height) = view_box_size(&svg).ok_or_else(|| Arc::from("RaTeX returned an SVG without a size"))?;
    // The bitmap GPUI would make for the SVG itself (`Image::to_image_data`); glyphs are paths, so
    // no fonts are loaded for it.
    let image = Image::from_bytes(ImageFormat::Svg, svg.into_bytes()).to_image_data(renderer.clone()).map_err(|err| Arc::from(format!("could not draw the formula: {err}")))?;
    Ok(Arc::new(Formula { image, width, height }))
}

/// Width and height from the root `viewBox`, in pixels. Neither SVG we draw has usable `width` and
/// `height` attributes: RaTeX writes them in points, which would draw a third too large, and merman
/// writes `width="100%"`.
pub(crate) fn view_box_size(svg: &str) -> Option<(f32, f32)> {
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

    fn draw(tex: &str, display: bool, font_size: f32, dark: bool) -> Rendered {
        render(tex, display, font_size, dark, &SvgRenderer::new(Arc::new(())))
    }

    #[test]
    fn formulas_render_and_broken_tex_reports_an_error() {
        let fraction = draw(r"\frac{n(n+1)}{2}", true, 16.0, true).unwrap();
        let inline = draw(r"\frac{n(n+1)}{2}", false, 16.0, true).unwrap();
        assert!(fraction.width > 0.0 && fraction.height > inline.height);
        assert!(draw(r"\frac{1}{", false, 16.0, true).is_err());
    }

    #[test]
    fn a_formula_is_laid_out_once_per_size_and_theme() {
        let first = draw("x^2", false, 15.0, false).unwrap();
        let again = draw("x^2", false, 15.0, false).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &draw("x^2", false, 15.0, true).unwrap()));
    }

    /// RaTeX runs in Marky's process and a release build aborts on a panic, so TeX cut off anywhere
    /// (as it is while someone types it) must come back as an error, never a panic.
    #[test]
    fn truncated_tex_never_panics() {
        let formulas = [
            r"\frac{n(n+1)}{2} + \sqrt[3]{x^2_{i,j}}",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}^{-1}",
            r"\begin{aligned} f(x) &= \int_0^\infty e^{-x}\,dx \\ &= \left[ -e^{-x} \right]_0^\infty \end{aligned}",
            r"\sum_{k=1}^{n} \binom{n}{k} \overbrace{x\cdots x}^{k} \color{red}{\mathbb{R}}",
            r"\text{if } \{ x \mid x > 0 \} \operatorname{arg\,max} \hat{\vec{v}} \underline{\overline{y}}",
        ];
        let renderer = SvgRenderer::new(Arc::new(()));
        for formula in formulas {
            for (end, _) in formula.char_indices().skip(1) {
                let _ = lay_out(&formula[..end], end % 2 == 0, 16.0, false, &renderer);
            }
        }
    }

    #[test]
    fn the_bitmap_is_drawn_at_twice_the_formula_size() {
        let formula = draw(r"\sqrt{x^2+y^2}", false, 16.0, false).unwrap();
        let bitmap = formula.image.size(0);
        assert!(bitmap.width.0 >= (formula.width * 2.0) as i32);
    }
}
