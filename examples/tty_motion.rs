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
///
/// `ZGUI_TTY_MOTION_FRAME` overrides it in milliseconds. A scene that finishes inside this interval
/// reports the interval rather than its own cost, which is what happens the moment something
/// expensive is ablated to find out what it was worth.
const FRAME: Duration = Duration::from_millis(16);

/// How often the frame rate is written to the log.
const REPORT: Duration = Duration::from_secs(1);

/// A scene with several things moving at once.
#[component]
fn Motion() -> impl IntoView {
    // The block's extent, so that a run can hold the number of blocks fixed and change only how
    // many pixels they cover — which is what separates a cost paid per draw from one paid per
    // pixel.
    let side = std::env::var("ZGUI_TTY_MOTION_SIZE")
        .ok()
        .and_then(|held| held.parse::<u32>().ok())
        .unwrap_or(64)
        .clamp(4, 512);
    let boxes = std::env::var("ZGUI_TTY_MOTION_BOXES")
        .ok()
        .and_then(|held| held.parse::<usize>().ok())
        .unwrap_or(BOXES)
        .clamp(1, 512);

    // The frame counter every position is derived from. One signal rather than one per block, so a
    // tick writes once and the blocks are recomputed from it.
    let tick = RwSignal::new(0_u64);
    let rate = RwSignal::new(String::from("measuring"));

    let started = Instant::now();
    let mut window = started;
    let mut drawn = 0_u64;
    let mut worst = Duration::ZERO;
    let mut last = started;
    let interval = std::env::var("ZGUI_TTY_MOTION_FRAME")
        .ok()
        .and_then(|held| held.parse::<u64>().ok())
        .map_or(FRAME, Duration::from_millis);
    let _timer = RwSignal::new_local(set_interval(interval, move || {
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
                        let travel = (cell_wide - side as f32 - 8.0).max(0.0);
                        let left = 24.0 + column as f32 * cell_wide + along * travel;
                        let top = 24.0 + row as f32 * cell_tall;
                        view! {
                            column(
                                class = "motion__box",
                                style = Some(format!(
                                    "left: {}px; top: {}px; width: {side}px; height: {}px",
                                    left as i32,
                                    top as i32,
                                    (side / 2).max(2),
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

/// The stylesheet. No gradients: what is being measured is the cost of moving things, and a
/// gradient would put the answer in the rasteriser instead.
///
/// The blocks are translucent, well rounded and cast a shadow, and each of those three costs
/// something different. A translucent fill has to be blended over what is under it, so the
/// backdrop is read as well as written and the frame cannot skip what the block covers — an opaque
/// fill lets both the erasure and everything beneath it be dropped. A large radius makes the
/// antialiased arc a larger share of the block than a small one does. A shadow is a second, softer
/// primitive that reaches outside the block's own box, so the damage a moving block owes is wider
/// than the block.
///
/// That is the point of them. A block that is a flat opaque rectangle takes every shortcut the
/// pipeline has, and measuring it says little about an interface that looks like an interface.
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
        border-radius: RADIUSpx;
        background-color: rgba(59, 108, 246, ALPHA);
        box-shadow: SHADOW;
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

/// The stylesheet with the three knobs on the block filled in from the environment.
///
/// The defaults are the taxing block that [`SHEET`] describes. Each knob turns off one of the three
/// costs separately, which is what tells them apart: a run cannot say whether the blending or the
/// shadow is what it is waiting on if it can only measure the two together.
///
/// * `ZGUI_TTY_MOTION_ALPHA` — the fill's opacity. `1` makes it opaque, and an opaque fill lets the
///   frame drop the erasure and everything the block covers.
/// * `ZGUI_TTY_MOTION_RADIUS` — the corner radius in pixels. `0` makes the block a plain rectangle.
/// * `ZGUI_TTY_MOTION_SHADOW` — the `box-shadow` value. `none` removes it, which is what shrinks
///   the ink a moving block owes back to the block itself.
fn sheet() -> String {
    fn knob(name: &str, fallback: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| fallback.to_owned())
    }
    SHEET
        .replace("RADIUS", &knob("ZGUI_TTY_MOTION_RADIUS", "14"))
        .replace("ALPHA", &knob("ZGUI_TTY_MOTION_ALPHA", "0.55"))
        .replace(
            "SHADOW",
            &knob("ZGUI_TTY_MOTION_SHADOW", "0 8px 22px rgba(0, 0, 0, 0.55)"),
        )
}

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
        .with_stylesheet(sheet());

    if std::env::var_os("ZGUI_TTY_WINDOWED").is_some() {
        return described.run(|| view! { Motion() });
    }
    described.run_drm(|| view! { Motion() })
}
