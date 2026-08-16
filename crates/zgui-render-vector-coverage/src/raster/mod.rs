//! The rasteriser: flatten on the host, multisample on the device, one resolve per scratch layer.

pub mod geometry;
pub mod instance;
pub mod pipeline;
pub mod scratch;

use std::sync::Arc;

use kurbo::Affine;
use zgui_geom::{Device, Matrix4, Rect};
use zgui_render::{
    Layering, MemoryReport, VectorError, VectorFrame, VectorPass, VectorPlan, VectorRaster,
    VectorTarget,
};
use zgui_render_wgpu::Gpu;
use zgui_render_wgpu::frame::vector::VectorSource;
use zgui_scene::{PaintTable, ScenePassPlan, VectorItem};

use crate::raster::geometry::Segment;
use crate::raster::instance::{Item, Run};
use crate::raster::pipeline::{Pipelines, Storage};
use crate::raster::scratch::Scratch;

/// What one frame's rasterisation cost and could not do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rasterised {
    /// Residual clip outlines applied inside the scratch.
    pub clip_layers: u32,
    /// Items left undrawn because a residual clip had no shape to apply.
    pub unclippable: u32,
    /// Items left undrawn because nothing here paints what they asked for.
    pub unpaintable: u32,
    /// Items whose transform is not two-dimensional, drawn without it.
    pub flattened_transforms: u32,
    /// Line segments the frame flattened.
    pub segments: u32,
}

/// A vector rasteriser that needs no compute shaders.
///
/// See the crate documentation for what switching to this costs; it is a visible downgrade and not
/// a transparent one.
#[derive(Debug)]
pub struct CoverageRaster {
    /// The device it draws on.
    gpu: Arc<Gpu>,
    /// The pipelines.
    pipelines: Pipelines,
    /// The two textures a pass passes through.
    scratch: Scratch,
    /// Every outline the fragment stage walks: each band's own segments, then each clip run's.
    outlines: Vec<Segment>,
    /// Per band: where its own segments start in `outlines`, and how many there are.
    bands: Vec<[u32; 4]>,
    /// One item's flattened outline, before it is cut into bands. Held to allocate nothing per item.
    flattened: Vec<Segment>,
    /// How many segments each band of the item being cut holds, then where each band's start. Held
    /// for the same reason.
    tally: Vec<u32>,
    /// Where each residual clip's outline is.
    runs: Vec<Run>,
    /// What to fill, in the order it is filled.
    items: Vec<Item>,
    /// Which instances belong to which pass.
    spans: Vec<(u32, u32)>,
    /// The regions the layering is computed from, kept so that a frame allocates nothing for it.
    regions: Vec<Rect<i32, Device>>,
    /// Which passes went into which layer, kept for the same reason.
    layered: Vec<Vec<usize>>,
    /// The buffers the three of those are uploaded to.
    buffers: Buffers,
    /// What the last frame cost and could not do.
    last: Rasterised,
    /// How many layers the last frame's passes needed.
    depth: u32,
}

/// Where one item's bands are, and what they cover.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Banding {
    /// The first band, in the band table.
    first: u32,
    /// How many bands there are; zero for a shape with nothing to fill.
    count: u32,
    /// The pixel boundary the first band starts at.
    top: f32,
    /// How many pixels tall one band is, which is a whole number and at least one.
    tall: f32,
}

/// The first and last band a segment reaches, both inclusive.
///
/// A segment belongs to every band its y-extent touches: one that ends inside a band and one that
/// crosses it whole are both crossings a ray may meet. A segment outside the bands altogether is
/// clamped into the nearest, where it crosses no sample row and so counts for nothing.
fn band_span(segment: Segment, top: f32, tall: f32, count: usize) -> (usize, usize) {
    let last = count.saturating_sub(1) as isize;
    let of = |y: f32| (((y - top) / tall).floor() as isize).clamp(0, last) as usize;
    (
        of(segment[1].min(segment[3])),
        of(segment[1].max(segment[3])),
    )
}

/// The device-side copies of the host arrays.
#[derive(Debug)]
struct Buffers {
    /// The items.
    items: Storage,
    /// Every outline the fragment stage walks.
    outlines: Storage,
    /// The clip runs.
    runs: Storage,
    /// Per band: where its own segments start, and how many.
    bands: Storage,
}

impl CoverageRaster {
    /// The most bands one shape may have, so a very tall shape cannot produce an unbounded table.
    ///
    /// A band is one pixel tall until a shape is taller than this, which no shape on a display of
    /// this generation is; past it the bands grow rather than the table.
    const MAX_BANDS: usize = 4096;

    /// A rasteriser on `gpu`, sized for a surface of `width` by `height` device pixels.
    pub fn new(gpu: &Arc<Gpu>, width: u32, height: u32) -> Self {
        let mut scratch = Scratch::new();
        scratch.ensure(gpu, width.max(1), height.max(1), Scratch::LAYERS);
        Self {
            pipelines: Pipelines::new(gpu),
            scratch,
            outlines: Vec::new(),
            bands: Vec::new(),
            flattened: Vec::new(),
            tally: Vec::new(),
            runs: Vec::new(),
            items: Vec::new(),
            spans: Vec::new(),
            regions: Vec::new(),
            layered: Vec::new(),
            buffers: Buffers {
                items: Storage::new(gpu, "zgui.vector.coverage.items"),
                outlines: Storage::new(gpu, "zgui.vector.coverage.outlines"),
                runs: Storage::new(gpu, "zgui.vector.coverage.runs"),
                bands: Storage::new(gpu, "zgui.vector.coverage.bands"),
            },
            gpu: Arc::clone(gpu),
            last: Rasterised::default(),
            depth: 0,
        }
    }

    /// What the last frame cost, and what it could not do.
    pub fn last_frame(&self) -> Rasterised {
        self.last
    }

    /// How many scratch layers the last frame's passes needed.
    ///
    /// The frame's own demand, which is what says how much of it overlapped — not how many layers
    /// are allocated, which never falls below a floor and follows the demand down only slowly.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// How many scratch layers are allocated, in each of the two textures.
    pub fn layers(&self) -> u32 {
        self.scratch.layers()
    }

    /// The extent every scratch layer is allocated at.
    pub fn extent(&self) -> (u32, u32) {
        self.scratch.extent()
    }

    /// The bytes of video memory both scratch textures occupy.
    pub fn scratch_bytes(&self) -> u64 {
        self.scratch.bytes()
    }

    /// Turns one pass's items into fills, and says how many there are.
    fn collect(&mut self, frame: &VectorFrame<'_>, pass: &VectorPass) -> (u32, u32) {
        let first = self.items.len() as u32;
        // The *layer's* extent, because the attachment a draw is mapped onto is the whole layer.
        let (layer_width, layer_height) = self.scratch.extent();
        let extent = [layer_width as f32, layer_height as f32, 0.0, 0.0];
        // Nothing at all: a layer is in the surface's own coordinates, which is what lets two passes
        // that do not meet on the screen share it. An outline is already in device space and stays
        // there.
        let shift = Affine::translate((
            f64::from(pass.raster_region.origin.x - pass.region.origin.x),
            f64::from(pass.raster_region.origin.y - pass.region.origin.y),
        ));
        let origin = pass.raster_region.origin;

        for planned in frame.plan.items_of(pass) {
            let Some(item) = frame.items.get(planned.item) else {
                continue;
            };
            let links = frame.clips.links(planned.residual);
            let Some(shapes) = links
                .iter()
                .map(zgui_scene::clip::path::of)
                .collect::<Option<Vec<_>>>()
            else {
                self.last.unclippable += 1;
                continue;
            };
            // The residual is a clip chain, so it is already in device space and the layer is too:
            // it is placed unchanged, never through the item's own transform.
            let clip_first = self.runs.len();
            for shape in &shapes {
                let start = self.outlines.len();
                geometry::flatten(shape, shift, &mut self.outlines);
                self.runs.push(Run::new(start, self.outlines.len() - start));
                self.last.segments += (self.outlines.len() - start) as u32;
                self.last.clip_layers += 1;
            }

            let placement = shift * self.transform_of(item, frame);
            // The item's own shape clips are in the item's own space, so unlike the residual they
            // go through the item's transform: a clipped drawing that is rotated has its clip
            // rotated with it.
            for clip in &item.clips {
                let start = self.outlines.len();
                geometry::flatten(&clip.path, placement, &mut self.outlines);
                self.runs.push(Run::of(
                    start,
                    self.outlines.len() - start,
                    clip.rule == peniko::Fill::EvenOdd,
                ));
                self.last.segments += (self.outlines.len() - start) as u32;
                self.last.clip_layers += 1;
            }
            let clip_count = self.runs.len() - clip_first;

            // The ink is recorded relative to its pass's region and the layer is in device
            // coordinates, so the two are added back together here.
            let bounds = [
                (origin.x + planned.ink.origin.x) as f32 - 1.0,
                (origin.y + planned.ink.origin.y) as f32 - 1.0,
                planned.ink.size.width as f32 + 2.0,
                planned.ink.size.height as f32 + 2.0,
            ];
            let mut painted = false;
            if let Some(color) = flat(item.fill, frame.paints) {
                self.flattened.clear();
                geometry::flatten(&item.path, placement, &mut self.flattened);
                let band = self.band(bounds);
                self.items.push(Item {
                    bounds,
                    viewport: extent,
                    color,
                    control: [
                        band.first as f32,
                        band.count as f32,
                        f32::from(u8::from(item.fill_rule == peniko::Fill::EvenOdd)),
                        clip_first as f32,
                    ],
                    bands: [clip_count as f32, band.top, band.tall, 0.0],
                });
                painted = true;
            }
            if let Some(stroke) = item.stroke.as_ref()
                && let Some(color) = flat(Some(stroke.paint), frame.paints)
            {
                self.flattened.clear();
                geometry::flatten_stroke(&item.path, &stroke.style, placement, &mut self.flattened);
                let band = self.band(bounds);
                self.items.push(Item {
                    bounds,
                    viewport: extent,
                    color,
                    // A stroke's outline is always filled by the non-zero rule whatever the fill
                    // rule of the shape it came from: the outline is a boundary, not a region the
                    // author wrote a rule for.
                    control: [band.first as f32, band.count as f32, 0.0, clip_first as f32],
                    bands: [clip_count as f32, band.top, band.tall, 0.0],
                });
                painted = true;
            }
            if !painted {
                self.last.unpaintable += 1;
            }
        }
        (first, self.items.len() as u32 - first)
    }

    /// Cuts the flattened outline into horizontal bands over `bounds`.
    ///
    /// A fragment then walks the handful of segments in its own band instead of every segment of
    /// the shape, which is the same answer reached by less arithmetic: a segment that does not
    /// cross the sample's row cannot cross the ray cast from it. Sixteen samples a pixel over a
    /// display-sized glyph is tens of millions of segment tests a frame, and this is what takes the
    /// multiplier out of it.
    ///
    /// The bands are a whole number of pixels tall and start on a pixel boundary, so a pixel lies
    /// wholly inside one of them. That is what lets a fragment find its band once and walk it once
    /// for all sixteen of its samples.
    ///
    /// Each band holds a copy of its own segments rather than indices into a shared table. A
    /// segment that spans several bands is written once for each, which costs the host a little
    /// memory and saves the device one texture fetch per segment per pixel.
    fn band(&mut self, bounds: [f32; 4]) -> Banding {
        // The pixel boundary at or above the box, because a band boundary has to be one too.
        let top = bounds[1].floor();
        let first = self.bands.len() as u32;
        self.last.segments += self.flattened.len() as u32;
        let height = bounds[1] + bounds[3] - top;
        // `is_finite` as well as the sign: a height that came out NaN would otherwise divide into
        // a band count of nothing, and the shape would be tested against no segments at all.
        if self.flattened.is_empty() || !height.is_finite() || height <= 0.0 {
            return Banding {
                first,
                count: 0,
                top,
                tall: 1.0,
            };
        }
        // One pixel a band, until the shape is taller than the table may be; then whole pixels
        // still, but more of them each.
        let tall = (height / Self::MAX_BANDS as f32).ceil().max(1.0);
        let count = ((height / tall).ceil() as usize).clamp(1, Self::MAX_BANDS);

        // Counted first and placed second, so that each band's segments end up next to each other
        // without a list per band. `tally` holds the count of each band, then where each starts.
        self.tally.clear();
        self.tally.resize(count, 0);
        for index in 0..self.flattened.len() {
            let (low, high) = band_span(self.flattened[index], top, tall, count);
            for slot in low..=high {
                self.tally[slot] += 1;
            }
        }
        let base = self.outlines.len();
        let mut running = 0;
        for slot in 0..count {
            let held = self.tally[slot];
            self.tally[slot] = running;
            self.bands.push([base as u32 + running, held, 0, 0]);
            running += held;
        }
        self.outlines.resize(base + running as usize, [0.0; 4]);
        for index in 0..self.flattened.len() {
            let segment = self.flattened[index];
            let (low, high) = band_span(segment, top, tall, count);
            for slot in low..=high {
                self.outlines[base + self.tally[slot] as usize] = segment;
                self.tally[slot] += 1;
            }
        }
        Banding {
            first,
            count: count as u32,
            top,
            tall,
        }
    }

    /// The item's own transform, counting the ones this cannot apply.
    fn transform_of(&mut self, item: &VectorItem, frame: &VectorFrame<'_>) -> Affine {
        let Some(id) = item.transform else {
            return Affine::IDENTITY;
        };
        let Some(matrix) = frame.placements.get(id) else {
            return Affine::IDENTITY;
        };
        if !matrix.is_2d() {
            self.last.flattened_transforms += 1;
            return Affine::IDENTITY;
        }
        affine_of(matrix)
    }

    /// Sorts the passes that were given a layer into the layer each one went into.
    fn group(&mut self, passes: &[VectorPass]) {
        let layers = (self.scratch.layers() as usize).max(1);
        for bucket in &mut self.layered {
            bucket.clear();
        }
        self.layered.resize_with(layers, Vec::new);
        for (index, pass) in passes.iter().enumerate() {
            if let Some(bucket) = self.layered.get_mut(pass.target.0 as usize) {
                bucket.push(index);
            }
        }
    }

    /// Records every pass of the frame into one encoder and submits it.
    ///
    /// One accumulation pass and one resolve per *layer*, not per pass: the passes sharing a layer
    /// are disjoint on the surface, so their outlines accumulate side by side and one draw converts
    /// the whole layer into what a composite reads. Resolving per pass would convert the same layer
    /// once per pass in it, and every conversion after the first would read what the one before it
    /// had already written.
    fn record(&self) -> Result<(), VectorError> {
        let bind = self
            .gpu
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zgui.vector.coverage"),
                layout: &self.pipelines.coverage_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.buffers.items.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.buffers.outlines.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.buffers.runs.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.buffers.bands.binding(),
                    },
                ],
            });
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zgui.vector.coverage"),
                });
        for (layer, indices) in self.layered.iter().enumerate() {
            if indices.is_empty() {
                continue;
            }
            let layer = layer as u32;
            let (Some(accumulation), Some(straight)) = (
                self.scratch.accumulation(layer),
                self.scratch.straight(layer),
            ) else {
                return Err(VectorError::Allocation {
                    detail: format!("no scratch layer {layer} was allocated"),
                });
            };
            {
                let mut render = begin(&mut encoder, accumulation, "zgui.vector.coverage");
                render.set_pipeline(&self.pipelines.coverage);
                render.set_bind_group(0, &bind, &[]);
                for &index in indices {
                    let (first, count) = self.spans[index];
                    if count > 0 {
                        render.draw(0..4, first..first + count);
                    }
                }
            }
            let resolve_bind = self
                .gpu
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("zgui.vector.coverage.resolve"),
                    layout: &self.pipelines.resolve_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(accumulation),
                    }],
                });
            let mut render = begin(&mut encoder, straight, "zgui.vector.coverage.resolve");
            render.set_pipeline(&self.pipelines.resolve);
            render.set_bind_group(0, &resolve_bind, &[]);
            render.draw(0..4, 0..1);
        }
        self.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }
}

impl VectorRaster for CoverageRaster {
    fn backend(&self) -> zgui_render::VectorBackend {
        zgui_render::VectorBackend::Coverage
    }

    fn plan(&mut self, passes: &ScenePassPlan) -> VectorPlan {
        if passes.is_empty() {
            return VectorPlan::empty();
        }
        let mut plan = VectorPlan::resourcing(passes);
        // Layers are shared by passes that do not meet on the surface: every pass is rasterised
        // before any of them is composited, so a layer holds its passes' coverage until their
        // composites have read it, and passes that do not overlap in device coordinates do not
        // overlap in a layer that is in device coordinates either.
        self.regions.clear();
        self.regions
            .extend(passes.passes.iter().map(|planned| planned.region));
        let layering = Layering::of(&self.regions, Scratch::MAX_LAYERS);
        let (packed, width, height) = layering.compact(&self.regions);
        self.depth = layering.layers();
        for (index, planned) in passes.passes.iter().enumerate() {
            plan.passes.push(VectorPass {
                region: planned.region,
                raster_region: packed[index],
                target: layering.target(index),
                items: planned.items.clone(),
                clip: planned.clip,
                instanced: planned.instanced,
            });
        }
        // The far corner of the surface anything is drawn at, not the largest region: a layer holds
        // device pixels where they belong, so it has to reach as far as the furthest of them.
        self.scratch
            .ensure(&self.gpu, width, height, layering.layers());
        plan
    }

    fn clear_targets(&mut self, plan: &VectorPlan) {
        let mut layers: Vec<u32> = plan
            .passes
            .iter()
            .filter(|pass| pass.target != VectorTarget::NONE)
            .map(|pass| pass.target.0 as u32)
            .collect();
        layers.sort_unstable();
        layers.dedup();
        self.scratch.clear(&self.gpu, &layers);
    }

    fn prepare(&mut self, frame: &mut VectorFrame<'_>) -> Result<(), VectorError> {
        self.last = Rasterised::default();
        self.outlines.clear();
        self.bands.clear();
        self.runs.clear();
        self.items.clear();
        self.spans.clear();
        if frame.is_empty() {
            return Ok(());
        }
        // The passes that were given a layer are a prefix, so the ones that were not are exactly the
        // tail a shortened plan drops — and a composite is named by its index, so it has to be a
        // tail and not a scattering.
        let prepared = frame
            .plan
            .passes
            .iter()
            .position(|pass| pass.target == VectorTarget::NONE)
            .unwrap_or(frame.plan.passes.len());
        {
            let _stage = tracing::debug_span!("cov.collect").entered();
            for index in 0..prepared {
                let pass = frame.plan.passes[index].clone();
                let span = self.collect(frame, &pass);
                self.spans.push(span);
            }
            self.group(&frame.plan.passes[..prepared]);
        }
        {
            let _stage = tracing::debug_span!(
                "cov.upload",
                items = self.items.len(),
                outlines = self.outlines.len(),
                bands = self.bands.len(),
                runs = self.runs.len()
            )
            .entered();
            self.buffers.items.upload(&self.gpu, &self.items);
            self.buffers.outlines.upload(&self.gpu, &self.outlines);
            self.buffers.runs.upload(&self.gpu, &self.runs);
            self.buffers.bands.upload(&self.gpu, &self.bands);
        }
        {
            let _stage = tracing::debug_span!("cov.record").entered();
            self.record()?;
        }
        if self.last.unclippable > 0 {
            tracing::warn!(
                items = self.last.unclippable,
                "vector items left undrawn because a residual clip had no shape to apply"
            );
        }
        if prepared < frame.plan.passes.len() {
            // More passes stacked over one point than there are layers to keep them apart. Reporting
            // it is what makes this frame's vector content missing rather than jumbled: the
            // alternative is two overlapping passes on one layer, and then one composite draws the
            // other's outlines.
            return Err(VectorError::OutOfCapacity {
                detail: format!(
                    "a frame planned {} passes and {} of them could be given one of {} layers",
                    frame.plan.passes.len(),
                    prepared,
                    Scratch::MAX_LAYERS
                ),
                prepared,
            });
        }
        Ok(())
    }

    fn memory(&self) -> MemoryReport {
        MemoryReport {
            // Nothing fixed at all, which is the whole shape of this rasteriser against the other
            // one: it holds two scratch textures and three buffers, and not one byte that does not
            // scale with what is drawn.
            fixed: 0,
            scratch: self.scratch.bytes(),
            buffers: self.buffers.items.capacity()
                + self.buffers.outlines.capacity()
                + self.buffers.runs.capacity()
                + self.buffers.bands.capacity(),
            ..MemoryReport::ZERO
        }
    }

    fn release_idle_resources(&mut self) -> u64 {
        let mut freed = self.scratch.release();
        freed += self.buffers.items.shrink(&self.gpu);
        freed += self.buffers.outlines.shrink(&self.gpu);
        freed += self.buffers.runs.shrink(&self.gpu);
        freed += self.buffers.bands.shrink(&self.gpu);
        self.items.clear();
        self.items.shrink_to_fit();
        self.outlines.clear();
        self.outlines.shrink_to_fit();
        self.runs.clear();
        self.runs.shrink_to_fit();
        self.flattened.shrink_to_fit();
        self.tally.shrink_to_fit();
        self.bands.shrink_to_fit();
        freed
    }
}

impl VectorSource for CoverageRaster {
    fn view(&self, target: VectorTarget) -> Option<&wgpu::TextureView> {
        self.scratch.straight(target.0 as u32)
    }
}

/// Opens a render pass that keeps what the attachment already holds.
fn begin<'encoder>(
    encoder: &'encoder mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
) -> wgpu::RenderPass<'encoder> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                // What is already there is this frame's pre-clear, and outlines composite over each
                // other, so it is kept rather than cleared again.
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// The straight, gamma-encoded colour a paint reference is drawn in, or `None` for one this
/// cannot draw at all.
fn flat(reference: Option<zgui_scene::PaintRef>, paints: &PaintTable) -> Option<[f32; 4]> {
    let entry = paints.get(reference?.id()?)?;
    // A ramp needs a per-fragment evaluation this deliberately does not have, so it is filled with
    // its mean colour: the shape still appears, which is the difference between a gradient-filled
    // icon looking flat on a device with no compute shaders and it not being there at all. A
    // sampled image has no such stand-in and is still not drawn.
    let color = entry.flat_color()?;
    let srgb = color.to_space(zgui_color::ColorSpace::Srgb);
    let [red, green, blue] = srgb.components();
    Some([red, green, blue, srgb.alpha()])
}

/// The two-dimensional affine a matrix embeds.
fn affine_of(matrix: &Matrix4) -> Affine {
    let column = matrix.columns;
    Affine::new([
        f64::from(column[0][0]),
        f64::from(column[0][1]),
        f64::from(column[1][0]),
        f64::from(column[1][1]),
        f64::from(column[3][0]),
        f64::from(column[3][1]),
    ])
}
