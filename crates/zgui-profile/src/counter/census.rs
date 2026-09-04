//! What one frame moved, written into the latency trace beside the stages that moved it.
//!
//! A stage's duration says a frame spent two milliseconds publishing fragments. It does not say
//! whether that was ten thousand fragments at two hundred nanoseconds each or a hundred at twenty
//! microseconds, and the two want opposite work: the first wants the walk to visit fewer, the
//! second wants each visit to cost less. Counters answer that, and the trace is where the durations
//! already are.
//!
//! Counters accumulate and nothing resets them per frame, so what is written is the **delta** since
//! the last census. The previous snapshot is held per thread, because a frame belongs to one.
//!
//! Off unless `ZGUI_COUNTERS` is set, and then it costs one snapshot and one formatted note a
//! frame. Nothing else reads it, so a build that never sets the variable never takes the snapshot.

use std::cell::RefCell;

use crate::counter::Counters;

thread_local! {
    /// What the counters read at the previous census on this thread.
    static PREVIOUS: RefCell<Counters> = const { RefCell::new(Counters::ZERO) };
}

/// Whether a census is being taken, from `ZGUI_COUNTERS`, read once.
fn asked() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var_os("ZGUI_COUNTERS").is_some())
}

/// Notes every counter that moved since the last census, under `stage`.
///
/// Only the ones that moved: a frame moves a dozen of the hundred and listing the rest would bury
/// them. Written as `name=delta` pairs so the line parses the way the other notes do.
pub fn note(stage: &'static str) {
    if !asked() {
        return;
    }
    let now = crate::counter::snapshot();
    let described = PREVIOUS.with_borrow_mut(|previous| {
        let mut described = String::new();
        for (counter, value) in now.iter() {
            let moved = value.saturating_sub(previous.get(counter));
            if moved == 0 {
                continue;
            }
            if !described.is_empty() {
                described.push(' ');
            }
            described.push_str(counter.name());
            described.push('=');
            described.push_str(&moved.to_string());
        }
        *previous = now;
        described
    });
    crate::latency::note(stage, described);
}
