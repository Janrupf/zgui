//! The draw-order entries every instanced batch of a frame is drawn from.

use crate::buffer::instances::StorageBuffer;
use crate::buffer::persist::OrderEntry;
use crate::buffer::upload::UploadBelt;
use crate::gpu::device::Gpu;
use crate::buffer::persist::LANES;

/// One frame's draw-order entries, staged while the frame is planned and uploaded once.
///
/// A damage rectangle is one replay of the batch stream under its own scissor, so a batch is drawn
/// once per rectangle with every one of its instances. The scissor then throws nearly all of them
/// away — and it throws them away *after* the vertex stage has read each one's whole record out of
/// the arena. Forty-eight rectangles over forty-eight moving boxes is two thousand three hundred
/// instances of which forty-eight put down a pixel; measured on the slowest device this runs on,
/// the ones that do not cost 0.61 microseconds each, which was a fifth of the frame.
///
/// So a rectangle stages the entries that can reach it and its draw names that run, rather than
/// every draw naming the whole of the list the scene resolved. Staged rather than written straight
/// through for the same reason every other per-frame block is: a frame needing more than the last
/// one reallocates, and a reallocation part way through would discard what had already been
/// written into the buffer it replaced.
///
/// Entries are staged as the *position* in a lane's draw order that survived, not as the arena slot
/// it resolves to, because the resolution is rebuilt while this frame's uploads are recorded —
/// after it is planned. Resolving at upload is what keeps the two in step; resolving while
/// planning reads the frame before, which is right only until something is inserted or removed.
#[derive(Debug)]
pub struct DrawOrders {
    /// This frame's entries as (lane, position), in the order they were planned.
    staged: Vec<(u8, u32)>,
    /// The entries those positions resolve to, rebuilt at upload.
    resolved: Vec<OrderEntry>,
    /// Where they are uploaded to.
    buffer: StorageBuffer,
}

impl DrawOrders {
    /// An empty list on `gpu`.
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            staged: Vec::new(),
            resolved: Vec::new(),
            buffer: StorageBuffer::vertex(gpu, "zgui.draw_orders"),
        }
    }

    /// Releases everything staged for the previous frame.
    pub fn begin_frame(&mut self) {
        self.staged.clear();
        self.resolved.clear();
    }

    /// Stages `positions` of `lane` and returns where they start and how many there are.
    pub fn stage(&mut self, lane: usize, positions: impl IntoIterator<Item = usize>) -> (u32, u32) {
        let first = self.staged.len() as u32;
        let lane = lane as u8;
        self.staged
            .extend(positions.into_iter().map(|at| (lane, at as u32)));
        (first, self.staged.len() as u32 - first)
    }

    /// Resolves this frame's positions through `resolved` and uploads them.
    ///
    /// A position no lane resolves is staged as the zero entry rather than dropped: the runs a
    /// plan already named have to keep the offsets they were given, and a slot nothing occupies
    /// draws a record of zeroes rather than another primitive's.
    pub fn upload_with(
        &mut self,
        gpu: &Gpu,
        belt: &mut UploadBelt,
        encoder: &mut wgpu::CommandEncoder,
        resolved: [&[OrderEntry]; LANES.len()],
    ) -> u64 {
        self.resolved.clear();
        self.resolved.extend(self.staged.iter().map(|(lane, at)| {
            resolved
                .get(usize::from(*lane))
                .and_then(|lane| lane.get(*at as usize))
                .copied()
                .unwrap_or_default()
        }));
        self.buffer.upload(gpu, belt, encoder, &self.resolved)
    }

    /// The buffer a draw binds as its instance input.
    pub fn buffer(&self) -> &wgpu::Buffer {
        self.buffer.buffer()
    }

    /// How many bytes are allocated on the device.
    pub fn bytes(&self) -> u64 {
        self.buffer.capacity()
    }

    /// How many entries this frame has staged.
    pub fn staged(&self) -> usize {
        self.staged.len()
    }
}
