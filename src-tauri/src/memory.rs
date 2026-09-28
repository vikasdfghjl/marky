//! A minimised window asks WebView2 to trim its memory, following Microsoft's guidance for
//! inactive webviews (`MemoryUsageTargetLevel`). Only on minimise, not on focus loss: returning
//! to normal may page memory back in, and Alt+Tab back into the editor should stay instant.
//! Tauri does not expose the setting, so this goes through the WebView2 controller.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, Window};

static MINIMIZED: AtomicBool = AtomicBool::new(false);

/// Called on every resize; acts only when the window enters or leaves the minimised state.
pub fn follow_minimized(window: &Window) {
    let minimized = window.is_minimized().unwrap_or(false);
    if MINIMIZED.swap(minimized, Ordering::Relaxed) == minimized {
        return;
    }
    let Some(webview) = window.app_handle().get_webview_window(window.label()) else { return };
    let _ = webview.with_webview(move |platform| set_low_memory(&platform.controller(), minimized));
}

fn set_low_memory(controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core::Interface;

    let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
    // SAFETY: plain COM calls on the live controller, made on the thread that owns it (with_webview).
    unsafe {
        let Ok(webview) = controller.CoreWebView2() else { return };
        // ICoreWebView2_19 needs WebView2 Runtime 114+; older runtimes simply keep normal memory use.
        let Ok(webview) = webview.cast::<ICoreWebView2_19>() else { return };
        let _ = webview.SetMemoryUsageTargetLevel(level);
    }
}
