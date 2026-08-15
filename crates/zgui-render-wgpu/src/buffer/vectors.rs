//! The instances every vector composite of a frame is drawn from.

use crate::buffer::tables::TableTexture;
use crate::gpu::device::Gpu;
use crate::pipeline::vector::VectorInstance;

/// One frame's vector-composite instances, staged while the frame is planned and uploaded once.
///
/// They are staged rather than written straight through for the same reason every other per-draw
/// block is: a frame that needs more of them than the last one reallocates, and a reallocation part
/// way through would discard whatever had already been written into the buffer it replaced.
#[derive(Debug)]
pub struct VectorInstances {
    /// This frame's instances, in the order they were planned.
    staged: Vec<VectorInstance>,
    /// Where they are uploaded to.
    buffer: TableTexture,
}

impl VectorInstances {
    /// An empty set of instances on `gpu`.
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            staged: Vec::new(),
            buffer: TableTexture::new(gpu, "zgui.vector_instances"),
        }
    }

    /// Releases everything staged for the previous frame.
    pub fn begin_frame(&mut self) {
        self.staged.clear();
    }

    /// Stages `instances` and returns the range of instance indices naming them.
    pub fn stage(&mut self, instances: impl IntoIterator<Item = VectorInstance>) -> (u32, u32) {
        let first = self.staged.len() as u32;
        self.staged.extend(instances);
        (first, self.staged.len() as u32 - first)
    }

    /// Uploads this frame's instances.
    pub fn upload_with(&mut self, gpu: &Gpu) -> u64 {
        self.buffer.upload(gpu, &self.staged)
    }

    /// The binding a bind group names.
    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        self.buffer.binding()
    }

    /// How many bytes are allocated on the device.
    pub fn bytes(&self) -> u64 {
        self.buffer.capacity()
    }

    /// The allocation epoch used by bind-group caches.
    pub fn generation(&self) -> u64 {
        self.buffer.generation()
    }

    /// Shrinks an oversized instance allocation and host staging vector.
    pub fn shrink(&mut self, gpu: &Gpu) -> u64 {
        self.staged.clear();
        self.staged.shrink_to_fit();
        self.buffer.shrink(gpu)
    }

    /// How many instances this frame has staged.
    pub fn staged(&self) -> usize {
        self.staged.len()
    }
}
