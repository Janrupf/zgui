//! A page of text redrawn every frame on a bare Linux console, and what the glyphs cost.
//!
//! `tty_motion.rs` answers what a scene of moving *boxes* costs, and its blocks are shadowed, which
//! turns out to be 88% of its frame. Neither it nor `tty.rs` says anything about glyphs: one draws
//! a clock that changes a digit a second, and the other draws a handful of characters beside
//! blocks that dominate everything.
//!
//! This one is glyphs and nothing else. Nothing here is rounded, translucent or shadowed, so what a
//! frame costs is the text.
//!
//! # What it draws
//!
//! A panel of [`LINES`] lines of prose, scrolled by one pixel a frame. Scrolled rather than
//! rewritten, because the two measure different halves: rewriting the text reshapes it and measures
//! the text stack, while moving it leaves every glyph in the atlas where it was and re-*draws* it.
//! What is under measurement here is the second — the sprite pipelines, which read a tile and blend
//! coverage.
//!
//! Whole pixels rather than fractions, for the same reason. A fractional offset changes each
//! glyph's subpixel phase, which is a different raster and a different atlas entry, and the frame
//! then measures the rasteriser and the atlas rather than the drawing.
//!
//! The panel is deliberately smaller than the screen. A full-screen page damages everything, and
//! then the copy to the scanout buffer — which crosses a bus on the machine this was written for —
//! is so much larger than the drawing that a change in the drawing cannot be seen in the frame
//! rate.
//!
//! # What it reports
//!
//! As `tty_motion.rs`: a line a second naming the frames drawn and the median frame time, with the
//! stage marks under `ZGUI_LATENCY`. `drawn_px` in the `sub.out` note is the glyph area, and
//! `presented_px` the copy, so the two can be told apart afterwards.
//!
//! # Running it
//!
//! It needs a free virtual terminal, for the reasons `tty.rs` sets out — the keyboard is grabbed and
//! there is no `SIGINT`. `Escape` quits. `ZGUI_TTY_TEXT_LINES` and `ZGUI_TTY_TEXT_SIZE` override how
//! much text there is and how large it is drawn.

use std::time::{Duration, Instant};

use zgui::prelude::*;

/// How many lines of text the panel holds.
///
/// Enough to fill the panel twice over, so that scrolling never reaches the end of them.
const LINES: usize = 64;

/// How often the scene is advanced, matching nearly every display this would run on.
const FRAME: Duration = Duration::from_millis(16);

/// How often the frame rate is written to the log.
const REPORT: Duration = Duration::from_secs(1);

/// The prose the lines are drawn from.
///
/// Ordinary words of varying length rather than one repeated character, so the glyph set is the one
/// a real page uses and the atlas holds what a real page holds.
const WORDS: &[&str] = &[
    "the",
    "frame",
    "carries",
    "every",
    "glyph",
    "again",
    "because",
    "a",
    "sprite",
    "reads",
    "its",
    "tile",
    "and",
    "blends",
    "coverage",
    "over",
    "whatever",
    "lies",
    "beneath",
    "it",
    "which",
    "is",
    "what",
    "this",
    "measures",
    "on",
    "a",
    "console",
    "with",
    "no",
    "compositor",
    "under",
    "it",
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

/// A page of text scrolling a pixel a frame.
#[component]
fn Text() -> impl IntoView {
    let lines = std::env::var("ZGUI_TTY_TEXT_LINES")
        .ok()
        .and_then(|held| held.parse::<usize>().ok())
        .unwrap_or(LINES)
        .clamp(1, 512);

    // The frame counter the scroll offset is derived from, as in `tty_motion.rs`: one signal that a
    // tick writes, and everything else recomputed from it.
    let tick = RwSignal::new(0_u64);
    let rate = RwSignal::new(String::from("measuring"));

    let started = Instant::now();
    let mut window = started;
    let mut drawn = 0_u64;
    let mut worst = Duration::ZERO;
    let mut last = started;
    let _timer = RwSignal::new_local(set_interval(FRAME, move || {
        let now = Instant::now();
        let took = now.saturating_duration_since(last);
        last = now;
        drawn += 1;
        worst = worst.max(took);
        tick.update(|frame| *frame += 1);

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
            column(class = "page__window") {
                column(
                    class = "page__scroll",
                    // One pixel a frame, wrapping well before the end of the lines so the panel is
                    // never short of text. Whole pixels: see the head of this file.
                    style = move || Some(format!("top: -{}px", tick.get() % 600)),
                ) {
                    {rows
                        .iter()
                        .map(|text| view! { label(class = "page__line") {{text.clone()}} })
                        .collect::<Vec<_>>()}
                }
            }
        }
    }
}

/// The stylesheet. Nothing rounded, translucent or shadowed anywhere: each of those is a different
/// pipeline, and this exists to measure one.
const SHEET: &str = r"
    :root {
        width: 100%;
        height: 100%;
        background-color: #05070c;
        color: #e8ecf4;
        font-family: monospace;
        display: flex;
        align-items: center;
        justify-content: center;
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

    .page__window {
        position: absolute;
        left: 24px;
        top: 24px;
        width: 880px;
        height: 560px;
        overflow: hidden;
        background-color: #0b0f18;
    }

    .page__scroll {
        position: absolute;
        left: 0;
        width: 880px;
    }

    .page__line {
        font-size: SIZEpx;
        line-height: 20px;
        color: #cbd6e8;
    }
";

/// The stylesheet with the one knob filled in.
///
/// `ZGUI_TTY_TEXT_SIZE` is the font size in pixels. It is the knob that separates a cost paid per
/// glyph from one paid per pixel: the glyph count holds still while the area each covers changes.
fn sheet() -> String {
    let size = std::env::var("ZGUI_TTY_TEXT_SIZE").unwrap_or_else(|_| "14".to_owned());
    SHEET.replace("SIZE", &size)
}

/// Sends the log to a file, because on this console standard error is the screen.
fn log() {
    let path =
        std::env::var("ZGUI_TTY_LOG").unwrap_or_else(|_| "/tmp/zgui-tty-text.log".to_owned());
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
        .with_application_id("dev.zgui.TtyText")
        .with_title("a page of text redrawn every frame")
        .with_stylesheet(sheet());

    if std::env::var_os("ZGUI_TTY_WINDOWED").is_some() {
        return described.run(|| view! { Text() });
    }
    described.run_drm(|| view! { Text() })
}
