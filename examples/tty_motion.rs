//! Several things moving at once on a bare Linux console, and what that costs.
//!
//! `tty.rs` draws a clock, which changes once a second — a good proof that a console backend works
//! and a poor measure of what it can sustain. This one asks the harder question: how many frames a
//! second does a scene with **several independent animations** reach on the machine it is running
//! on, and what is the frame time when it does.
//!
//! # What it draws
//!
//! [`BOXES`] blocks **spread over the whole screen**, each on its own path at its own speed, and a
//! bar whose width follows the frame counter. Each block is absolutely positioned, so what moves is
//! one offset and a frame damages the rectangle a block left and the one it arrived at, and nothing
//! else. That is the shape of an interface animating rather than of a benchmark redrawing
//! everything, and it is the shape a damage-limited backend is built for.
//!
//! Spread over the screen rather than stacked in a column, because the two ask different questions
//! of the backend. Stacked, every block is its own band of rows and the damage is a tall thin
//! column; spread, the blocks share rows and the damage is scattered. A backend that copies rows
//! and a backend that copies rectangles answer those two very differently.
//!
//! # What it reports
//!
//! The frame counter on the screen, and a line per second on the log naming the frames drawn in it
//! and the median frame time. `ZGUI_LATENCY` carries the stage marks for the same run, so a slow
//! frame can be taken apart afterwards.
//!
//! **Frames a second is the number here**, unlike in `tty.rs` where the scene is paced at one hertz
//! and the frame time is all that means anything. This asks for a frame as often as it can get one.
//!
//! # Running it
//!
//! It needs a free virtual terminal, for the reasons `tty.rs` sets out at length — the keyboard is
//! grabbed and there is no `SIGINT`. `Escape` quits. `ZGUI_TTY_MOTION_BOXES` overrides how many
//! blocks there are, for finding where a machine's ceiling is.

use std::time::{Duration, Instant};

use zgui::prelude::*;

/// How many blocks move at once.
///
/// Six is enough that no one of them dominates the damage and few enough that the whole set fits
/// the width of a small display. `ZGUI_TTY_MOTION_BOXES` overrides it.
const BOXES: usize = 6;

/// How often the scene is advanced.
///
/// Sixteen milliseconds, which is the refresh interval of nearly every display this would run on.
/// Asking for frames faster than the screen can show them measures the loop rather than the
/// picture.
const FRAME: Duration = Duration::from_millis(16);

/// How often the frame rate is written to the log.
const REPORT: Duration = Duration::from_secs(1);

/// A scene with several things moving at once.
#[component]
fn Motion() -> impl IntoView {
    let boxes = std::env::var("ZGUI_TTY_MOTION_BOXES")
        .ok()
        .and_then(|held| held.parse::<usize>().ok())
        .unwrap_or(BOXES)
        .clamp(1, 64);

    // The frame counter every position is derived from. One signal rather than one per block, so a
    // tick writes once and the blocks are recomputed from it.
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

    view! {
        column(
            class = "motion",
            // The keyboard is grabbed, so this is the only way out.
            on:key_down = move |ev| {
                if matches!(ev.key, Key::Named(NamedKey::Escape)) {
                    use_windows().quit();
                }
            },
        ) {
            label(class = "motion__rate") {{move || rate.get()}}
            column(class = "motion__stage") {
                {move || (0..boxes)
                    .map(|which| {
                        // Each block on its own path and its own period, so nothing lines up and
                        // the damage is scattered the way a real interface's is. The rows are laid
                        // out so that the set fills the screen however many there are.
                        let frame = tick.get();
                        let columns = ((boxes as f32).sqrt().ceil() as usize).max(1);
                        let rows = boxes.div_ceil(columns);
                        let (column, row) = (which % columns, which / columns);
                        let period = 90 + which as u64 * 17;
                        let phase = (frame + which as u64 * 13) % period;
                        let along = if phase * 2 < period {
                            phase as f32 / (period as f32 / 2.0)
                        } else {
                            2.0 - phase as f32 / (period as f32 / 2.0)
                        };
                        // The cell this block travels inside, and how far along it is.
                        let cell_wide = 1180.0 / columns as f32;
                        let cell_tall = 940.0 / rows.max(1) as f32;
                        let left = 24.0 + column as f32 * cell_wide + along * (cell_wide - 72.0);
                        let top = 24.0 + row as f32 * cell_tall;
                        view! {
                            column(
                                class = "motion__box",
                                style = Some(format!(
                                    "left: {}px; top: {}px",
                                    left as i32, top as i32
                                )),
                            )
                        }
                    })
                    .collect::<Vec<_>>()}
            }
            row(class = "motion__bar") {
                column(
                    class = "motion__fill",
                    style = move || Some(format!("width: {}px", tick.get() % 400 + 8)),
                )
            }
        }
    }
}

/// The stylesheet. Flat colours and no gradients: what is being measured is the cost of moving
/// things, and a gradient would put the answer in the rasteriser instead.
const SHEET: &str = r"
    :root {
        width: 100%;
        height: 100%;
        background-color: #05070c;
        color: #e8ecf4;
        font-family: sans-serif;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .motion {
        position: absolute;
        left: 0;
        top: 0;
        width: 1280px;
        height: 1024px;
        background-color: #0b0f18;
    }

    .motion__rate {
        position: absolute;
        left: 24px;
        top: 990px;
        font-size: 18px;
        color: #7f8ca6;
    }

    .motion__stage {
        position: absolute;
        left: 0;
        top: 0;
        width: 1280px;
        height: 1024px;
    }

    .motion__box {
        position: absolute;
        width: 64px;
        height: 26px;
        border-radius: 6px;
        background-color: #3b6cf6;
    }

    .motion__bar {
        position: absolute;
        left: 24px;
        top: 972px;
        width: 420px;
        height: 10px;
        border-radius: 5px;
        background-color: #141a26;
    }

    .motion__fill {
        height: 10px;
        border-radius: 5px;
        background-color: #4ad19a;
    }
";

/// Sends the log to a file, because on this console standard error is the screen.
fn log() {
    let path =
        std::env::var("ZGUI_TTY_LOG").unwrap_or_else(|_| "/tmp/zgui-tty-motion.log".to_owned());
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
        .with_application_id("dev.zgui.TtyMotion")
        .with_title("several things moving at once")
        .with_stylesheet(SHEET);

    if std::env::var_os("ZGUI_TTY_WINDOWED").is_some() {
        return described.run(|| view! { Motion() });
    }
    described.run_drm(|| view! { Motion() })
}
