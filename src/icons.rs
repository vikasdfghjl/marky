//! Toolbar icons. GPUI paints an SVG as a monochrome mask, so the shapes are black and the button tints them.
//! The editor pane and preview pane are the same shapes as the app's toolbar.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

const EDITOR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="1.5" fill="none" stroke="black"/><path d="M2 3h6v10H2z" fill="black"/><path d="M8 3v10" stroke="black"/></svg>"#;
const EDITOR_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="1.5" fill="none" stroke="black"/><path d="M8 3v10" stroke="black"/></svg>"#;
const PREVIEW: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="1.5" fill="none" stroke="black"/><path d="M8 3h6v10H8z" fill="black"/><path d="M8 3v10" stroke="black"/></svg>"#;
const PREVIEW_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="1.5" fill="none" stroke="black"/><path d="M8 3v10" stroke="black"/></svg>"#;
const OUTLINE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3 4.5h10M3 8h10M5 11.5h8" fill="none" stroke="black" stroke-linecap="round"/></svg>"#;

pub struct IconAssets;

impl AssetSource for IconAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let svg = match path {
            "icons/editor.svg" => EDITOR,
            "icons/editor-off.svg" => EDITOR_OFF,
            "icons/preview.svg" => PREVIEW,
            "icons/preview-off.svg" => PREVIEW_OFF,
            "icons/outline.svg" => OUTLINE,
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
