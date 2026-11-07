//! GPUI keeps every bitmap it has drawn until it is told to drop it, and every edit of a formula or
//! a diagram makes a new one. The preview's caches make their bitmaps themselves (instead of handing
//! GPUI an SVG `Image`, whose bitmap only GPUI can reach) so they can drop the ones no longer shown.

use std::sync::Arc;

use gpui::{App, RenderImage};

/// Drops `bitmaps` from every window's texture atlas. Deferred so that no window is borrowed by
/// the update that called it: `drop_image` then reaches every window.
pub fn drop_bitmaps(bitmaps: Vec<Arc<RenderImage>>, cx: &mut App) {
    if bitmaps.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for bitmap in bitmaps {
            cx.drop_image(bitmap, None);
        }
    });
}
