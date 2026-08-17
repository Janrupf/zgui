//! Waiting for a frame the card is still drawing, away from the frame loop.
//!
//! A driver that exports no sync file leaves this program to wait for its own drawing, and that
//! wait is the whole time the card takes — 22 ms of a 27 ms frame on the slowest machine this runs
//! on. A loop that waits for it is a loop that answers no key, fires no timer and reads no page
//! flip while it does, which is most of a frame.
//!
//! So the wait happens on a thread of its own. The loop hands over the fence
//! ([`Placed`](crate::import::gl::Placed)), keeps turning, and learns that the frame is drawn when
//! the thread writes to the wake channel it is already parked on. Nothing here touches the graphics
//! device: an EGL sync object belongs to its display rather than to a context, so waiting on one
//! needs nothing current.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use crate::import::gl::Placed;

/// What one wake adds to the loop's counter.
const ONE: u64 = 1;

/// A frame handed to the waiting thread, and whether it has been drawn yet.
///
/// One per frame, because a display may have more than one frame on the card at a time: the flag
/// travels with the frame rather than with the display, so a frame that finishes tells no other
/// frame's story.
#[derive(Clone, Debug)]
pub(crate) struct Watched(Arc<AtomicBool>);

impl Watched {
    /// Whether the card has finished drawing this frame.
    pub(crate) fn drawn(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The thread that waits for fences, and the channel that reaches it.
///
/// One per display. It lives as long as the display does and stops when the sender is dropped,
/// which is what closes the channel and ends the thread's loop.
#[derive(Debug)]
pub(crate) struct Waiter {
    /// What the thread is handed, and the flag to set when it has waited.
    hand: Sender<(Placed, Watched)>,
    /// Kept so the thread is joined when the display goes, rather than left running.
    thread: Option<JoinHandle<()>>,
}

impl Waiter {
    /// Starts the thread that waits, waking the loop through `wake` when a frame is drawn.
    ///
    /// `wake` is a descriptor of the loop's own wake channel. The thread writes a count to it and
    /// nothing else: the loop is parked on that descriptor already, and a wake with no reason
    /// behind it ends the park and dispatches nothing — which is exactly what is wanted, because
    /// what the loop does next is look at its displays.
    pub(crate) fn new(wake: OwnedFd) -> Self {
        let (hand, taken) = channel::<(Placed, Watched)>();
        let thread = std::thread::Builder::new()
            .name("zgui-drm-fence".to_owned())
            .spawn(move || {
                for (placed, watched) in taken {
                    placed.settle();
                    // Set before the wake, so a loop that the write brings back finds the flag.
                    watched.0.store(true, Ordering::Release);
                    // A failure here is a channel that is closing, which is the program stopping.
                    // The frame is drawn either way and the flag says so.
                    let _ = rustix::io::write(&wake, &ONE.to_ne_bytes());
                }
            })
            .ok();
        Self { hand, thread }
    }

    /// Hands `placed` to the thread, and answers the flag that frame will be marked on.
    ///
    /// Answers nothing where the thread is gone, which is a machine that could not start one. The
    /// caller waits for itself there, the way it did before there was a thread.
    pub(crate) fn watch(&self, placed: Placed) -> Option<Watched> {
        let watched = Watched(Arc::new(AtomicBool::new(false)));
        match self.hand.send((placed, watched.clone())) {
            Ok(()) => Some(watched),
            Err(_) => None,
        }
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        // The sender goes first: the thread's loop ends when the channel closes, and joining before
        // that would wait for a thread that is still waiting for a frame.
        let (hand, _) = channel();
        drop(core::mem::replace(&mut self.hand, hand));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}
