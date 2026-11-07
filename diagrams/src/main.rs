//! Draws Mermaid diagrams for the Marky GPUI prototype. merman is about 10 MB, so it lives in this
//! separate program, which the prototype starts only when a document shows a diagram (as the app
//! loads Mermaid only then). DECISION.md has the reasons and sizes.
//!
//! Protocol: one JSON request per line on stdin, `{"source": "...", "dark": false}`, and one JSON
//! reply per line on stdout, `{"svg": "..."}` or `{"error": "..."}`, in order. The program exits
//! when stdin closes (the prototype quit or was killed) or after a minute without a request, so an
//! idle prototype holds no second process; the prototype restarts it on the next diagram.

use std::io::{BufRead, BufWriter, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use merman::{Engine, MermaidConfig, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use serde::{Deserialize, Serialize};

const IDLE_EXIT: Duration = Duration::from_secs(60);

/// merman checks this deadline cooperatively; the prototype kills the process if it is ignored.
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Request {
    source: String,
    dark: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Reply {
    Svg(String),
    Error(String),
}

fn main() {
    // Lines arrive on a thread so the main loop can stop after a quiet minute.
    let (lines, requests) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let mut renderers: [Option<Renderer>; 2] = [None, None];
    let mut out = BufWriter::new(std::io::stdout().lock());
    loop {
        let line = match requests.recv_timeout(IDLE_EXIT) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
        };
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let renderer = renderers[usize::from(request.dark)].get_or_insert_with(|| renderer(request.dark));
                draw(renderer, &request.source)
            }
            Err(err) => Reply::Error(format!("bad request: {err}")),
        };
        let written = serde_json::to_writer(&mut out, &reply).is_ok() && out.write_all(b"\n").is_ok() && out.flush().is_ok();
        if !written {
            return;
        }
    }
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

fn draw(renderer: &Renderer, source: &str) -> Reply {
    let control = OperationControl::new().with_deadline(DEADLINE);
    match renderer.render(RenderRequest::svg(source, control, SvgRequest::default())) {
        Ok(RenderOutput::Svg(Some(svg))) => Reply::Svg(svg.svg().to_string()),
        Ok(_) => Reply::Error("not a diagram Mermaid knows".to_string()),
        Err(err) => Reply::Error(err.to_string()),
    }
}
