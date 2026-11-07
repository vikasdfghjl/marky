//! Mermaid diagrams for the preview. merman renders them in-process; diagrams are drawn on the
//! background thread only when the preview shows one, and the SVG is rasterised here so unused
//! bitmaps can be dropped with the rest of the preview's images.
//!
//! Only diagrams the preview draws are requested (the preview is a virtualised list), each once per
//! source and theme: later frames and typing elsewhere reuse the cached result.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Image, ImageFormat, RenderImage, SvgRenderer};
use merman::{Engine, MermaidConfig, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

use crate::{bitmaps, perf};

/// merman checks this deadline cooperatively; a diagram that ignores it is reported as failed.
const DEADLINE: Duration = Duration::from_secs(5);

/// A diagram as a bitmap (at GPUI's 2× SVG scale) and its natural size in pixels.
pub struct Diagram {
    pub image: Arc<RenderImage>,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub enum State {
    Drawing,
    Drawn(Arc<Diagram>),
    Failed(Arc<str>),
}

/// The source and dark theme: everything the drawing depends on.
type Key = (String, bool);

fn cache() -> &'static Mutex<HashMap<Key, State>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, State>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

thread_local! {
    /// One renderer per theme per worker thread. Building it parses Mermaid's configuration, so
    /// keep it for every diagram that thread draws.
    static RENDERERS: RefCell<[Option<Renderer>; 2]> = const { RefCell::new([None, None]) };
}

/// The diagram's state for this frame. The first time a source is seen it starts drawing it on a
/// background thread and redraws the view when it is done; until then it reports `Drawing`.
pub fn state<V: 'static>(source: &str, dark: bool, cx: &mut Context<V>) -> State {
    let key: Key = (source.to_string(), dark);
    {
        let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(state) = cache.get(&key) {
            return state.clone();
        }
        cache.insert(key.clone(), State::Drawing);
    }
    let request = key.clone();
    let renderer = cx.svg_renderer();
    let requested = Instant::now();
    let drawing = cx.background_spawn(async move { draw(&request.0, request.1, &renderer) });
    cx.spawn(async move |view, cx| {
        let state = drawing.await;
        perf::log(&format!("diagram_ms={:.0} at={:.0}", requested.elapsed().as_secs_f64() * 1000.0, perf::since_start_ms()));
        cache().lock().unwrap_or_else(PoisonError::into_inner).insert(key, state);
        let _ = view.update(cx, |_, cx| cx.notify());
    })
    .detach();
    State::Drawing
}

/// Forgets every diagram that is not in `in_use` or not in the current theme, and drops its bitmap
/// from GPUI's texture atlas. Call it after the open documents change.
pub fn release_unused(in_use: &HashSet<&str>, dark: bool, cx: &mut App) {
    let mut released = Vec::new();
    cache().lock().unwrap_or_else(PoisonError::into_inner).retain(|(source, theme), state| {
        let keep = *theme == dark && in_use.contains(source.as_str());
        if let (false, State::Drawn(diagram)) = (keep, state) {
            released.push(diagram.image.clone());
        }
        keep
    });
    bitmaps::drop_bitmaps(released, cx);
}

/// The app's Mermaid settings: its light or dark theme and `securityLevel: 'strict'` (the source is
/// untrusted). Labels are SVG text instead of HTML in `<foreignObject>`, which only a browser draws.
fn renderer(dark: bool) -> Renderer {
    let config = MermaidConfig::from_value(serde_json::json!({
        "theme": if dark { "dark" } else { "default" },
        "securityLevel": "strict",
        "htmlLabels": false,
        "flowchart": { "htmlLabels": false },
    }));
    Renderer::new().with_engine(Engine::new().with_site_config(config))
}

fn draw(source: &str, dark: bool, svg_renderer: &SvgRenderer) -> State {
    let svg = RENDERERS.with(|renderers| {
        let mut renderers = renderers.borrow_mut();
        let renderer = renderers[usize::from(dark)].get_or_insert_with(|| renderer(dark));
        render_diagram(renderer, source)
    });
    match svg {
        Ok(svg) => rasterize(svg, svg_renderer),
        Err(err) => State::Failed(Arc::from(err)),
    }
}

fn render_diagram(renderer: &Renderer, source: &str) -> Result<String, String> {
    let control = OperationControl::new().with_deadline(DEADLINE);
    match renderer.render(RenderRequest::svg(source, control, SvgRequest::default())) {
        Ok(RenderOutput::Svg(Some(svg))) => Ok(svg.svg().to_string()),
        Ok(_) => Err("not a diagram Mermaid knows".to_string()),
        Err(err) => Err(err.to_string()),
    }
}

/// The bitmap GPUI would make for the SVG itself (`Image::to_image_data`), made here so it is ours to drop.
fn rasterize(svg: String, renderer: &SvgRenderer) -> State {
    let Some((width, height)) = crate::math::view_box_size(&svg) else { return failed("the diagram has no size") };
    match Image::from_bytes(ImageFormat::Svg, svg.into_bytes()).to_image_data(renderer.clone()) {
        Ok(image) => State::Drawn(Arc::new(Diagram { image, width, height })),
        Err(err) => failed(&format!("could not draw the SVG: {err}")),
    }
}

fn failed(message: &str) -> State {
    State::Failed(Arc::from(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> SvgRenderer {
        SvgRenderer::new(Arc::new(()))
    }

    #[test]
    fn a_drawn_diagram_keeps_its_natural_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 120 40"><rect width="120" height="40"/></svg>"#;
        let State::Drawn(diagram) = rasterize(svg.to_string(), &renderer()) else { panic!("expected a drawing") };
        assert_eq!((diagram.width, diagram.height), (120.0, 40.0));
    }

    #[test]
    fn draws_known_diagrams_and_reports_unknown_ones() {
        assert!(matches!(draw("flowchart LR\n  A --> B", false, &renderer()), State::Drawn(_)));
        assert!(matches!(draw("not a diagram", true, &renderer()), State::Failed(_)));
        assert!(matches!(draw("pie title P\n  \"a\" : 1", true, &renderer()), State::Drawn(_)));
    }
}
