//! Mermaid diagrams for the preview. merman draws them in a separate program, marky-diagrams.exe
//! (`diagrams/`), because it is about 10 MB; it starts only when a document shows a diagram, as the
//! app loads Mermaid only then. GPUI draws the SVG it returns. DECISION.md has the reasons and sizes.
//!
//! Only diagrams the preview draws are requested (the preview is a virtualised list), each once per
//! source and theme: later frames and typing elsewhere reuse the cached result.
//!
//! The bitmap is made here, on the background thread, so [`release_unused`] can drop the bitmaps of
//! diagrams that no open document contains any more (see `bitmaps.rs`).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Image, ImageFormat, RenderImage, SvgRenderer};
use serde::Deserialize;

use crate::{bitmaps, perf};

const HELPER: &str = "marky-diagrams.exe";

/// The helper gives up after 5 s through merman's own deadline; this catches a helper that hangs anyway.
const TIMEOUT: Duration = Duration::from_secs(10);

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
        // From the request to the bitmap, with the helper's start when it was not running.
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

/// The running helper. One request at a time: callers wait on the mutex.
fn helper() -> &'static Mutex<Option<Helper>> {
    static HELPER: Mutex<Option<Helper>> = Mutex::new(None);
    &HELPER
}

/// Asks the helper for one diagram, starting it if needed. A helper that was already running may
/// have quit after its idle minute or crashed on an earlier diagram, so it gets one restart.
fn draw(source: &str, dark: bool, renderer: &SvgRenderer) -> State {
    let Ok(request) = serde_json::to_string(&serde_json::json!({ "source": source, "dark": dark })) else {
        return failed("could not encode the request");
    };
    let reply = {
        let mut running = helper().lock().unwrap_or_else(PoisonError::into_inner);
        let mut restarted = running.is_none();
        loop {
            if running.is_none() {
                match Helper::start() {
                    Ok(started) => *running = Some(started),
                    Err(err) => return failed(&err),
                }
            }
            let Some(current) = running.as_mut() else { return failed("the diagram helper did not start") };
            match current.ask(&request) {
                Ok(reply) => break reply,
                Err(Stop::TimedOut) => {
                    *running = None;
                    return failed(&format!("drawing took longer than {} s", TIMEOUT.as_secs()));
                }
                Err(Stop::Exited) if !restarted => {
                    *running = None;
                    restarted = true;
                }
                Err(Stop::Exited) => {
                    *running = None;
                    return failed("the diagram helper stopped while drawing this diagram");
                }
            }
        }
    };
    match parse_reply(&reply) {
        Ok(svg) => rasterize(svg, renderer),
        Err(err) => State::Failed(err),
    }
}

/// The helper's reply line, `{"svg": "..."}` or `{"error": "..."}`.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Reply {
    Svg(String),
    Error(String),
}

fn parse_reply(line: &str) -> Result<String, Arc<str>> {
    match serde_json::from_str::<Reply>(line) {
        Ok(Reply::Svg(svg)) => Ok(svg),
        Ok(Reply::Error(err)) => Err(Arc::from(err)),
        Err(err) => Err(Arc::from(format!("unreadable reply from the diagram helper: {err}"))),
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

enum Stop {
    Exited,
    TimedOut,
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    /// Reply lines, read on a thread so a hung helper can time out.
    replies: Receiver<String>,
}

impl Helper {
    fn start() -> Result<Self, String> {
        let program = helper_path().ok_or_else(|| format!("{HELPER} was not found next to the program"))?;
        let mut command = Command::new(&program);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashes up for the helper.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|err| format!("could not start {HELPER}: {err}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(format!("could not connect to {HELPER}"));
        };
        let (lines, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, stdin, replies })
    }

    fn ask(&mut self, request: &str) -> Result<String, Stop> {
        let sent = self.stdin.write_all(request.as_bytes()).and_then(|()| self.stdin.write_all(b"\n")).and_then(|()| self.stdin.flush());
        if sent.is_err() {
            return Err(Stop::Exited);
        }
        match self.replies.recv_timeout(TIMEOUT) {
            Ok(reply) => Ok(reply),
            Err(RecvTimeoutError::Timeout) => Err(Stop::TimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(Stop::Exited),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Next to the program. Test binaries live one folder down, in `deps`.
fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?;
    for folder in [Some(folder), folder.parent()].into_iter().flatten() {
        let path = folder.join(HELPER);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> SvgRenderer {
        SvgRenderer::new(Arc::new(()))
    }

    #[test]
    fn replies_become_svgs_or_errors() {
        let svg = r#"{"svg":"<svg width=\"100%\" viewBox=\"0 0 615.5 185\"></svg>"}"#;
        assert!(parse_reply(svg).unwrap().contains("615.5"));
        assert_eq!(&*parse_reply(r#"{"error":"No Mermaid diagram type detected"}"#).unwrap_err(), "No Mermaid diagram type detected");
        assert!(parse_reply("not json").is_err());
    }

    #[test]
    fn a_drawn_diagram_keeps_its_natural_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 120 40"><rect width="120" height="40"/></svg>"#;
        let State::Drawn(diagram) = rasterize(svg.to_string(), &renderer()) else { panic!("expected a drawing") };
        assert_eq!((diagram.width, diagram.height), (120.0, 40.0));
    }

    /// Runs when the helper has been built (`cargo build --workspace`), as it is before a release.
    #[test]
    fn the_helper_draws_and_survives_a_bad_diagram() {
        if helper_path().is_none() {
            eprintln!("skipped: {HELPER} is not built");
            return;
        }
        assert!(matches!(draw("flowchart LR\n  A --> B", false, &renderer()), State::Drawn(_)));
        assert!(matches!(draw("not a diagram", true, &renderer()), State::Failed(_)));
        assert!(matches!(draw("pie title P\n  \"a\" : 1", true, &renderer()), State::Drawn(_)));
    }
}
