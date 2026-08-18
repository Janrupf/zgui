//! The one thing this crate does, stated so that a second device API can do it too.

use std::os::fd::OwnedFd;

use crate::buffer::Rect;
use crate::error::Error;

/// How a caller learns that a copy is done.
///
/// **A descriptor is worth asking for even where the copy is small.** It is the difference between
/// the kernel waiting and this program waiting: a descriptor goes to the atomic commit as the
/// plane's `IN_FENCE_FD`, or is merged with the renderer's own fence, and the thread that asked for
/// the copy goes back to work at once. Without one the wait has already been paid, on the thread
/// that called, before the answer came back.
#[derive(Debug)]
pub enum Signalled {
    /// A descriptor that becomes readable when the copy is done.
    ///
    /// Nothing has waited for it yet. Hand it to the commit, or merge it with the fence the frame's
    /// own drawing produced.
    Descriptor(OwnedFd),
    /// The copy is finished, because this device offered no descriptor and the wait happened here.
    ///
    /// Correct everywhere, and the reason a caller never has to ask what a device can do before
    /// asking it to copy.
    Waited,
}

impl Signalled {
    /// Takes the descriptor, where there is one.
    pub fn descriptor(self) -> Option<OwnedFd> {
        match self {
            Self::Descriptor(fd) => Some(fd),
            Self::Waited => None,
        }
    }
}

/// A device that can copy rectangles between the buffers it has been given.
///
/// A trait rather than one type because the device API is the part that varies. The GL
/// implementation is [`egl`](crate::egl); a Vulkan one would import the same descriptors through
/// `VK_EXT_external_memory_dma_buf`, copy with `vkCmdCopyImage`, and — the reason it is worth
/// writing — hand back a real descriptor on hardware whose EGL cannot, through
/// `VK_KHR_external_fence_fd`.
pub trait Copier {
    /// Copies each of `rects` from the buffer at `from` to the buffer at `to`.
    ///
    /// The rectangles are in buffer coordinates and mean the same region in both, because that is
    /// what a repair is: the same pixels, at the same place, in a buffer that is behind. An empty
    /// rectangle is skipped, and an empty list is a copy of nothing that still answers.
    ///
    /// Nothing here waits for the copy where the device can be asked for a descriptor instead —
    /// see [`Signalled`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Driver`] where the device refused, and [`Error::NoCopy`] where it offers no
    /// way to copy at all. A slot number outside the set it was given is [`Error::Driver`] as well,
    /// because it is the caller's arithmetic that was wrong and the message has to say so.
    fn copy(&mut self, from: usize, to: usize, rects: &[Rect]) -> Result<Signalled, Error>;

    /// How many buffers this copier holds.
    fn len(&self) -> usize;

    /// Returns `true` where it holds none.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
