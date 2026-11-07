//! Windows high-contrast themes. GPUI reports only light or dark, so Marky reads the system colours
//! itself and maps its light/dark pairs onto them while high contrast is on.
//!
//! The mapping is deliberately coarse: text becomes `COLOR_WINDOWTEXT` (or `COLOR_HOTLIGHT` for
//! accents), surfaces become `COLOR_WINDOW` or `COLOR_BTNFACE`, and selections and find highlights
//! become `COLOR_HIGHLIGHT`. Exact shades are the theme's job, not the app's.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{Rgba, rgb};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

#[derive(Clone, Copy, Debug, Default)]
pub struct Palette {
    pub window: u32,
    pub window_text: u32,
    pub button_face: u32,
    pub highlight: u32,
    pub gray_text: u32,
    pub hotlight: u32,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static WINDOW: AtomicU32 = AtomicU32::new(0);
static WINDOW_TEXT: AtomicU32 = AtomicU32::new(0);
static BUTTON_FACE: AtomicU32 = AtomicU32::new(0);
static HIGHLIGHT: AtomicU32 = AtomicU32::new(0);
static GRAY_TEXT: AtomicU32 = AtomicU32::new(0);
static HOTLIGHT: AtomicU32 = AtomicU32::new(0);
static LAST_REFRESH: AtomicU32 = AtomicU32::new(0);

fn palette() -> Palette {
    let get = |value: &AtomicU32| value.load(Ordering::Relaxed);
    Palette {
        window: get(&WINDOW),
        window_text: get(&WINDOW_TEXT),
        button_face: get(&BUTTON_FACE),
        highlight: get(&HIGHLIGHT),
        gray_text: get(&GRAY_TEXT),
        hotlight: get(&HOTLIGHT),
    }
}

/// Reads the current theme. Called at startup and at most once a second from the frame loop, so a
/// theme switch applies without restarting the app.
pub fn refresh() {
    let mut contrast = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            &mut contrast as *mut HIGHCONTRASTW as *mut core::ffi::c_void,
            0,
        )
    };
    let active = ok != 0 && contrast.dwFlags & HCF_HIGHCONTRASTON != 0;
    ACTIVE.store(active, Ordering::Relaxed);
    if !active {
        return;
    }
    let color = |index| from_colorref(unsafe { GetSysColor(index) });
    WINDOW.store(color(COLOR_WINDOW), Ordering::Relaxed);
    WINDOW_TEXT.store(color(COLOR_WINDOWTEXT), Ordering::Relaxed);
    BUTTON_FACE.store(color(COLOR_BTNFACE), Ordering::Relaxed);
    HIGHLIGHT.store(color(COLOR_HIGHLIGHT), Ordering::Relaxed);
    GRAY_TEXT.store(color(COLOR_GRAYTEXT), Ordering::Relaxed);
    HOTLIGHT.store(color(COLOR_HOTLIGHT), Ordering::Relaxed);
}

pub fn refresh_periodically() {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_secs() as u32).unwrap_or(0);
    if LAST_REFRESH.swap(seconds, Ordering::Relaxed) != seconds {
        refresh();
    }
}

/// The colour to use for a light/dark pair, or `None` while high contrast is off.
pub fn color(dark_value: u32, light_value: u32) -> Option<Rgba> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    Some(rgb(map_with(&palette(), dark_value, light_value)))
}

/// The caret, which should be as visible as the theme's own text.
pub fn caret() -> Option<Rgba> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    Some(rgb(WINDOW_TEXT.load(Ordering::Relaxed)))
}

/// Maps one light/dark pair onto the system palette. Pure, so the rules can be tested without a
/// high-contrast theme installed.
fn map_with(palette: &Palette, dark_value: u32, light_value: u32) -> u32 {
    let dark_luminance = luminance(dark_value);
    let light_luminance = luminance(light_value);
    let dark_is_surface = dark_luminance < 0.35;
    let light_is_surface = light_luminance > 0.65;
    let accent = blueish(dark_value) || blueish(light_value);

    if accent {
        // A blue selection is a background; blue text is a link or the focus ring.
        return if dark_is_surface && light_is_surface { palette.highlight } else { palette.hotlight };
    }
    if dark_is_surface && light_is_surface {
        // Both are backgrounds. The main panes become the window; chrome, code and borders become
        // button faces. A vivid one (a find match) becomes the highlight.
        if is_vivid(dark_value) || is_vivid(light_value) {
            palette.highlight
        } else if dark_value > 0x202020 {
            palette.button_face
        } else {
            palette.window
        }
    } else if !dark_is_surface && !light_is_surface {
        // Both are text: the light value is the light theme's text, the dark value the dark theme's.
        if dark_luminance < 0.5 && light_luminance < 0.5 {
            palette.gray_text
        } else {
            palette.window_text
        }
    } else {
        // A border or a pair that swaps roles: keep it visible.
        palette.window_text
    }
}

fn luminance(value: u32) -> f32 {
    let red = ((value >> 16) & 0xFF) as f32;
    let green = ((value >> 8) & 0xFF) as f32;
    let blue = (value & 0xFF) as f32;
    (0.299 * red + 0.587 * green + 0.114 * blue) / 255.0
}

fn blueish(value: u32) -> bool {
    let red = ((value >> 16) & 0xFF) as i32;
    let green = ((value >> 8) & 0xFF) as i32;
    let blue = (value & 0xFF) as i32;
    blue > red + 40 && blue > green + 20
}

fn is_vivid(value: u32) -> bool {
    let red = ((value >> 16) & 0xFF) as i32;
    let green = ((value >> 8) & 0xFF) as i32;
    let blue = (value & 0xFF) as i32;
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    max - min > 60
}

/// `COLORREF` is `0x00BBGGRR`; the app's colours are `0xRRGGBB`.
fn from_colorref(value: u32) -> u32 {
    let red = (value & 0xFF) << 16;
    let green = value & 0xFF00;
    let blue = (value >> 16) & 0xFF;
    red | green | blue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette {
            window: 0x000000,
            window_text: 0xffffff,
            button_face: 0x303030,
            highlight: 0x00ffff,
            gray_text: 0x808080,
            hotlight: 0x00a0ff,
        }
    }

    #[test]
    fn text_and_surfaces_take_the_system_colours() {
        let palette = palette();
        assert_eq!(map_with(&palette, 0xfafafa, 0x18181b), palette.window_text);
        assert_eq!(map_with(&palette, 0x18181b, 0xf4f4f5), palette.window);
        assert_eq!(map_with(&palette, 0x27272a, 0xe4e4e7), palette.button_face);
    }

    #[test]
    fn selections_and_links_are_highlighted() {
        let palette = palette();
        assert_eq!(map_with(&palette, 0x1e3a8a, 0xbfdbfe), palette.highlight);
        assert_eq!(map_with(&palette, 0x58a6ff, 0x0969da), palette.hotlight);
        assert_eq!(map_with(&palette, 0x713f12, 0xfef08a), palette.highlight);
    }

    #[test]
    fn colorref_is_converted_to_rgb() {
        assert_eq!(from_colorref(0x000000FF), 0xFF0000);
        assert_eq!(from_colorref(0x00FF0000), 0x0000FF);
    }
}
