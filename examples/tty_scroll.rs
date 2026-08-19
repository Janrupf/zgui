//! A list scrolled a pixel a frame on a bare Linux console, through the container that scrolls it.
//!
//! `tty_text.rs` looks like this and is not this. There a panel of prose is moved by animating `top`
//! on a child, which reshapes nothing and re-*draws* everything — it measures the sprite pipelines,
//! which is what it was built for. **A real scroll takes a different path through the engine**: the
//! frame reports one container moved by a whole number of pixels, the fragments under it are
//! recorded as a rigid move rather than as new geometry, and a renderer that keeps its composed
//! target can then *copy* the surviving pixels where they now belong and draw only the strip the
//! copy left behind.
//!
//! Nothing measured that path on the machine this was written for, and it is the one a real
//! application spends its time in: a list, a document, a log. So this scene scrolls the container.
//!
//! # What it draws
//!
//! [`LINES`] lines of prose in a scrolling region [`PORT`] pixels tall, scrolled by one pixel a
//! frame with `scroll_to`, wrapping back to the top before it reaches the end. Nothing is rounded,
//! translucent or shadowed, so a frame is the text and the movement.
//!
//! Whole pixels, for the reason `tty_text.rs` gives about subpixel phase — and here for a second
//! reason: a fractional displacement is refused as a shift, because a whole-pixel copy cannot
//! express it.
//!
//! # What it reports
//!
//! As `tty_motion.rs`: a line a second naming the frames drawn and the median frame time, with the
//! stage marks under `ZGUI_LATENCY`. `r.shift` in the trace is the copy this scene exists to
//! provoke — its absence means the frame took the ordinary path and redrew the whole port.
//!
//! # Running it
//!
//! It needs a free virtual terminal, for the reasons `tty.rs` sets out. `Escape` quits.
//! `ZGUI_TTY_SCROLL_LINES` and `ZGUI_TTY_SCROLL_SIZE` override how much text there is and how large
//! it is drawn, and `ZGUI_TTY_SCROLL_FRAME` how often it advances.

use std::time::{Duration, Instant};

use zgui::prelude::*;
use zgui::view::scroll::{ScrollBehavior, ScrollTarget};

/// How many lines the list holds.
const LINES: usize = 200;

/// How tall the scrolling region is, in pixels.
const PORT: i32 = 560;

/// How far the list is scrolled before it wraps back to the top.
///
/// Short of the end, so that the wrap is the only discontinuity and every other frame is a movement
/// of one pixel.
const TRAVEL: f32 = 2000.0;

/// How often the scene is advanced, matching nearly every display this would run on.
///
/// `ZGUI_TTY_SCROLL_FRAME` overrides it in milliseconds. A scroll frame costs so little here that
/// the scene sits at this interval with the loop idle between ticks — so measuring what one really
/// costs means asking for them faster than a display could show them.
const FRAME: Duration = Duration::from_millis(16);

/// How often the frame rate is written to the log.
const REPORT: Duration = Duration::from_secs(1);

/// The prose the lines are drawn from.
const WORDS: &[&str] = &[
    "the",
    "list",
    "moves",
    "by",
    "one",
    "pixel",
    "and",
    "the",
    "pixels",
    "it",
    "keeps",
    "are",
    "already",
    "on",
    "the",
    "device",
    "so",
    "a",
    "frame",
    "that",
    "copies",
    "them",
    "draws",
    "only",
    "the",
    "strip",
    "it",
    "uncovered",
    "which",
    "is",
    "what",
    "this",
    "measures",
];

/// One line of text, of roughly `width` characters, seeded by `which` so no two lines are alike.
fn line(which: usize, width: usize) -> String {
    let mut text = String::with_capacity(width + 16);
    let mut at = which * 7;
    while text.len() < width {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(WORDS[at % WORDS.len()]);
        at += 1;
    }
    text
}

/// A list scrolled a pixel a frame, through the container that scrolls.
#[component]
fn Scrolling() -> impl IntoView {
    let lines = std::env::var("ZGUI_TTY_SCROLL_LINES")
        .ok()
        .and_then(|held| held.parse::<usize>().ok())
        .unwrap_or(LINES)
        .clamp(1, 2000);

    let rate = RwSignal::new(String::from("measuring"));
    let interval = std::env::var("ZGUI_TTY_SCROLL_FRAME")
        .ok()
        .and_then(|held| held.parse::<u64>().ok())
        .map_or(FRAME, Duration::from_millis);
    // The container itself, so that the tick below can scroll it rather than restyle anything.
    let port = NodeRef::new();

    let started = Instant::now();
    let mut window = started;
    let mut drawn = 0_u64;
    let mut worst = Duration::ZERO;
    let mut last = started;
    let mut at = 0.0_f32;
    let _timer = RwSignal::new_local(set_interval(interval, move || {
        let now = Instant::now();
        let took = now.saturating_duration_since(last);
        last = now;
        drawn += 1;
        worst = worst.max(took);

        // One whole pixel, which is what a shift can express. The wrap is the one frame in two
        // thousand that is not a movement.
        at = if at >= TRAVEL { 0.0 } else { at + 1.0 };
        port.scroll_to(
            ScrollTarget::Offset(zgui::geom::Point::new(
                zgui::geom::DevicePx(0.0),
                zgui::geom::DevicePx(at),
            )),
            ScrollBehavior::Instant,
        );

        if now.saturating_duration_since(window) >= REPORT {
            let seconds = now.saturating_duration_since(window).as_secs_f64();
            let each = seconds / drawn.max(1) as f64;
            tracing::info!(
                frames = drawn,
                per_second = format!("{:.1}", drawn as f64 / seconds),
                mean_ms = format!("{:.1}", each * 1000.0),
                worst_ms = format!("{:.1}", worst.as_secs_f64() * 1000.0),
                "motion"
            );
            rate.set(format!(
                "{:.0} fps, {:.1} ms a frame",
                drawn as f64 / seconds,
                each * 1000.0
            ));
            window = now;
            drawn = 0;
            worst = Duration::ZERO;
        }
    }));

    let rows: Vec<String> = (0..lines).map(|which| line(which, 78)).collect();

    view! {
        column(
            class = "page",
            on:key_down = move |ev| {
                if matches!(ev.key, Key::Named(NamedKey::Escape)) {
                    use_windows().quit();
                }
            },
        ) {
            label(class = "page__rate") {{move || rate.get()}}
            column(class = "page__port", node_ref = port) {
                {rows
                    .iter()
                    .map(|text| view! { label(class = "page__line") {{text.clone()}} })
                    .collect::<Vec<_>>()}
            }
        }
    }
}

/// The stylesheet. `overflow: scroll` on the port is what makes this a scroll rather than a move.
const SHEET: &str = r"
    :root {
        width: 100%;
        height: 100%;
        background-color: #05070c;
        color: #e8ecf4;
        font-family: monospace;
    }

    .page {
        position: absolute;
        left: 0;
        top: 0;
        width: 1280px;
        height: 1024px;
        background-color: #0b0f18;
    }

    .page__rate {
        position: absolute;
        left: 24px;
        top: 990px;
        font-size: 18px;
        color: #7f8ca6;
    }

    .page__port {
        position: absolute;
        left: 24px;
        top: 24px;
        width: 880px;
        height: PORTpx;
        overflow: scroll;
        background-color: #0b0f18;
    }

    .page__line {
        font-size: SIZEpx;
        line-height: 20px;
        color: #cbd6e8;
    }
";

/// The stylesheet with its two knobs filled in.
fn sheet() -> String {
    let size = std::env::var("ZGUI_TTY_SCROLL_SIZE").unwrap_or_else(|_| "14".to_owned());
    SHEET
        .replace("SIZE", &size)
        .replace("PORT", &PORT.to_string())
}

/// Sends the log to a file, because on this console standard error is the screen.
fn log() {
    let path =
        std::env::var("ZGUI_TTY_LOG").unwrap_or_else(|_| "/tmp/zgui-tty-scroll.log".to_owned());
    let Ok(file) = std::fs::File::create(&path) else {
        return;
    };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .finish();
    drop(tracing::subscriber::set_global_default(subscriber));
}

fn main() -> Result<(), zgui::Error> {
    log();
    let described = app()
        .with_application_id("dev.zgui.TtyScroll")
        .with_title("a list scrolled a pixel a frame")
        .with_stylesheet(sheet());

    if std::env::var_os("ZGUI_TTY_WINDOWED").is_some() {
        return described.run(|| view! { Scrolling() });
    }
    described.run_drm(|| view! { Scrolling() })
}
