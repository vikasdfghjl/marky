//! Asks WebView2 to trim its memory (`MemoryUsageTargetLevel`, Microsoft's guidance for inactive
//! webviews) while the window is minimised or the user is idle, at the cost of a slower first
//! keystroke afterwards (measured in the README's Benchmarks).
//!
//! Two reasons, one level: Low while either holds. Minimising is detected here; idleness is judged
//! by `idle.ts`, which reports only its transitions, so typing costs no IPC. Not on focus loss:
//! Alt+Tab back must stay instant. Tauri does not expose the setting, hence the WebView2 controller.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, Window};

static MINIMIZED: AtomicBool = AtomicBool::new(false);
static IDLE: AtomicBool = AtomicBool::new(false);
static LOW: AtomicBool = AtomicBool::new(false);

/// Called on every resize; acts only when the window enters or leaves the minimised state.
pub fn follow_minimized(window: &Window) {
    let minimized = window.is_minimized().unwrap_or(false);
    if MINIMIZED.swap(minimized, Ordering::Relaxed) != minimized {
        apply(window.app_handle());
    }
}

/// The frontend saw no input for a while (`true`), or input again (`false`).
pub fn set_idle(app: &AppHandle, idle: bool) {
    if IDLE.swap(idle, Ordering::Relaxed) != idle {
        apply(app);
    }
}

fn apply(app: &AppHandle) {
    let low = MINIMIZED.load(Ordering::Relaxed) || IDLE.load(Ordering::Relaxed);
    if LOW.swap(low, Ordering::Relaxed) == low {
        return; // e.g. going idle while already minimised
    }
    let Some(webview) = app.get_webview_window("main") else { return };
    let _ = webview.with_webview(move |platform| set_low_memory(&platform.controller(), low));
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
