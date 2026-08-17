//! Recording a planned frame into one command encoder.

use std::collections::BTreeMap;

use zgui_geom::{Device, Rect};
use zgui_profile::{Counter, counter};
use zgui_scene::ExternalTextureId;

use crate::atlas_backend::sink::AtlasTextures;
use crate::frame::build::FramePlan;
use crate::frame::segment::{EncoderOp, PassLoad, PlannedDraw, PlannedPass};
use crate::frame::target::TargetRef;
use crate::gpu::device::Gpu;
use crate::pipeline::Pipelines;
use crate::pipeline::kind::PipelineKind;
use crate::renderer::frame::FrameBuffers;
use crate::target::group_pool::GroupPool;
use crate::target::scene_texture::SceneTexture;

/// What one frame's recording produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    /// How many draw calls were issued.
    pub draw_calls: u32,
    /// How many planned draws could not be issued, for want of a pipeline or a texture.
    pub dropped: u32,
}

/// A texture the renderer was handed rather than one it drew.
pub struct AttachedTexture {
    /// What it is.
    pub texture: zgui_render::ExternalTexture,
    /// A view of it.
    pub view: wgpu::TextureView,
}

/// Everything a planned frame is recorded against.
pub struct Recorder<'frame> {
    /// The device.
    pub gpu: &'frame Gpu,
    /// The pipelines, built on demand.
    pub pipelines: &'frame mut Pipelines,
    /// The frame's buffers.
    pub buffers: &'frame FrameBuffers,
    /// The atlas textures.
    pub atlas: &'frame AtlasTextures,
    /// The pool isolated targets were lent by.
    pub pool: &'frame GroupPool,
    /// The persistent target the frame composes into.
    pub composed: &'frame SceneTexture,
    /// The sampler every magnifying read goes through.
    pub sampler: &'frame wgpu::Sampler,
    /// Textures the renderer did not draw.
    pub externals: &'frame BTreeMap<ExternalTextureId, AttachedTexture>,
    /// Whatever rasterised this frame's vector content, when there is one.
    pub vectors: Option<&'frame dyn crate::frame::vector::VectorSource>,
}

impl Recorder<'_> {
    /// Records `plan` into `encoder`.
    ///
    /// One pass is opened per pass segment and dropped before the next segment, because a live
    /// pass holds the encoder borrowed and the operations between passes need it. That constraint
    /// is not worked around here — it is what the plan is a plan *of*.
    pub fn record(&mut self, encoder: &mut wgpu::CommandEncoder, plan: &FramePlan) -> Recorded {
        use crate::frame::segment::Segment;
        let mut recorded = Recorded::default();
        let mut index = 0;
        while index < plan.segments.len() {
            match &plan.segments[index] {
                Segment::Encoder(op) => {
                    self.run(encoder, *op);
                    index += 1;
                }
                Segment::Pass(first) => {
                    // A damage rectangle is planned as a pass of its own, and on a frame that
                    // changed many places that is many passes over one attachment differing in
                    // nothing but their scissor. Opening a render pass is not free — on the driver
                    // this was measured against it costs about 140 microseconds, so a frame with
                    // forty-eight of them spends most of its time opening them — and none of that
                    // buys anything here, because what separates the rectangles is the scissor and
                    // a scissor can be set inside one pass as often as one likes.
                    //
                    // So a run of passes that agree on everything else is opened once. A pass that
                    // *discards* its attachment breaks the run: that clear covers the whole
                    // attachment however the scissor is set, so folding it into the pass before it
                    // would throw away what that one had just drawn.
                    let mut end = index + 1;
                    while let Some(Segment::Pass(next)) = plan.segments.get(end) {
                        if !shares_a_pass(first, next) {
                            break;
                        }
                        end += 1;
                    }
                    let run =
                        plan.segments[index..end]
                            .iter()
                            .filter_map(|segment| match segment {
                                Segment::Pass(pass) => Some(pass),
                                Segment::Encoder(_) => None,
                            });
                    self.record_pass(encoder, plan, run, &mut recorded);
                    index = end;
                }
            }
        }
        counter::add(Counter::DrawCalls, u64::from(recorded.draw_calls));
        if recorded.dropped > 0 {
            tracing::debug!(
                dropped = recorded.dropped,
                "planned draws the device could not issue"
            );
        }
        recorded
    }

    /// Runs one operation that needs the encoder itself.
    fn run(&self, encoder: &mut wgpu::CommandEncoder, op: EncoderOp) {
        match op {
            EncoderOp::Capture {
                source,
                destination,
                region,
            } => {
                let Some(from) = self.texture(source) else {
                    return;
                };
                let Some(to) = self.texture(destination) else {
                    return;
                };
                let region = clamp(region, from, to);
                if region.is_empty() {
                    return;
                }
                encoder.copy_texture_to_texture(
                    texel_copy(from, region),
                    texel_copy(to, region),
                    wgpu::Extent3d {
                        width: region.size.width as u32,
                        height: region.size.height as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    /// Opens one pass for a run of planned passes that share it, and issues all their draws.
    ///
    /// Every planned pass in `run` writes the same attachment under the same globals, so what
    /// distinguishes them is the scissor.
    fn record_pass<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &FramePlan,
        run: impl Iterator<Item = &'a PlannedPass> + Clone,
        recorded: &mut Recorded,
    ) {
        let Some(planned) = run.clone().next() else {
            return;
        };
        let Some(view) = self.view(planned.target) else {
            return;
        };
        let Some(format) = self.format(planned.target) else {
            return;
        };
        let extent = self.extent(planned.target);
        // Resolved before the pass is opened, because a run every scissor of which is empty draws
        // nothing anywhere and should not open one at all.
        let mut passes: Vec<&PlannedPass> = Vec::new();
        let mut scissors: Vec<Rect<i32, Device>> = Vec::new();
        for held in run {
            let scissor = scaled(held.scissor, held.target, extent);
            if scissor.is_empty() {
                continue;
            }
            passes.push(held);
            scissors.push(scissor);
        }
        if passes.is_empty() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zgui.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match planned.load {
                        PassLoad::Keep => wgpu::LoadOp::Load,
                        PassLoad::Discard => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // A target is allocated at a size *class*, so it is usually larger than the region it
        // holds. The viewport maps device coordinates onto the region rather than onto the whole
        // allocation; without it everything would be drawn at the wrong scale.
        pass.set_viewport(
            0.0,
            0.0,
            extent.width.max(1) as f32,
            extent.height.max(1) as f32,
            0.0,
            1.0,
        );
        let tables = self
            .buffers
            .frame_bind_group(self.gpu, self.pipelines.layouts());
        // Setting a pipeline and its bindings costs far more than setting a scissor — measured on
        // the driver this was built against, about twenty microseconds against a fraction of one —
        // so a run whose rectangles all replay the same draws issues each of those draws once and
        // sweeps the rectangles inside it. A frame damaged in forty-eight places then sets state
        // five times rather than two hundred and forty.
        //
        // Only when the rectangles are disjoint, which is what makes the two orders the same
        // picture: no pixel is written under more than one of them, so it sees its own rectangle's
        // draws in their planned order either way. Overlapping rectangles are drawn one at a time,
        // because a pixel in two of them has to be cleared again between the two.
        let mut run = Vec::with_capacity(scissors.len());
        match shared_shape(plan, &passes, &scissors) {
            Some(template) => {
                // Where each rectangle stands in the template. A rectangle draws a subsequence of
                // it — the kinds no instance of its own reaches are simply absent — so a cursor is
                // what says whether this rectangle is one of the ones the draw about to be issued
                // belongs to.
                let mut at = vec![0_usize; passes.len()];
                for shape in template {
                    run.clear();
                    for (index, (planned, scissor)) in passes.iter().zip(&scissors).enumerate() {
                        let Some(draw) = plan.draws_of(planned).get(at[index]) else {
                            continue;
                        };
                        if !same_shape(draw, shape) {
                            continue;
                        }
                        run.push((*scissor, draw));
                        at[index] += 1;
                    }
                    if run.is_empty() {
                        continue;
                    }
                    match self.issue(&mut pass, passes[0], tables.as_ref(), shape, format, &run) {
                        Some(issued) => recorded.draw_calls += issued,
                        None => recorded.dropped += 1,
                    }
                }
            }
            None => {
                for (planned, scissor) in passes.iter().zip(&scissors) {
                    for draw in plan.draws_of(planned) {
                        run.clear();
                        run.push((*scissor, draw));
                        match self.issue(&mut pass, planned, tables.as_ref(), draw, format, &run) {
                            Some(issued) => recorded.draw_calls += issued,
                            None => recorded.dropped += 1,
                        }
                    }
                }
            }
        }
    }

    /// Issues one planned draw over `run`, and answers how many draws that took.
    ///
    /// `draw` is the shape every entry of `run` shares — the same kind, drawn by the same pipeline
    /// — and is what the state is set from. What each entry carries of its own is which instances
    /// reach its rectangle. Answers nothing where the draw could not be issued at all, which is a
    /// pipeline or a binding the device has not got.
    fn issue(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        planned: &PlannedPass,
        tables: Option<&wgpu::BindGroup>,
        draw: &PlannedDraw,
        format: wgpu::TextureFormat,
        run: &[Swept<'_>],
    ) -> Option<u32> {
        match draw {
            PlannedDraw::Clear => {
                let pipeline = self
                    .pipelines
                    .get(self.gpu, PipelineKind::DamageClear, format)?;
                pass.set_pipeline(pipeline);
                Some(sweep(pass, run, once))
            }
            PlannedDraw::Instances { kind, texture, .. } => {
                self.instances(pass, planned, tables, *kind, *texture, format, run)
            }
            PlannedDraw::Shaded { shader, params, .. } => {
                self.shaded(pass, planned, tables, *shader, *params, format, run)
            }
            PlannedDraw::Blur {
                source,
                params,
                downsample,
            } => {
                let kind = if *downsample {
                    PipelineKind::BlurDownsample
                } else {
                    PipelineKind::BlurAxis
                };
                self.textured(pass, kind, *source, *params, None, format, run)
            }
            PlannedDraw::Composite { source, params } => self.textured(
                pass,
                PipelineKind::Composite,
                *source,
                *params,
                tables.map(|bind| (bind, planned.globals)),
                format,
                run,
            ),
            PlannedDraw::Effect {
                source,
                shader,
                params,
                block,
            } => self.effect_filter(pass, *source, *shader, *params, *block, format, run),
            PlannedDraw::Vector { target, .. } => {
                let view = self.vectors.and_then(|source| source.view(*target))?;
                let bind =
                    self.buffers
                        .vector_bind_group(self.gpu, self.pipelines.layouts(), view)?;
                let tables = tables?;
                let pipeline =
                    self.pipelines
                        .get(self.gpu, PipelineKind::VectorComposite, format)?;
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, tables, &[planned.globals]);
                pass.set_bind_group(1, &bind, &[]);
                Some(sweep(pass, run, composited))
            }
            PlannedDraw::External { texture, params } => {
                let attached = self.externals.get(texture)?;
                let bind = self.buffers.filtered_bind_group(
                    self.gpu,
                    self.pipelines.layouts(),
                    &attached.view,
                    self.sampler,
                )?;
                let tables = tables?;
                let pipeline = self
                    .pipelines
                    .get(self.gpu, PipelineKind::External, format)?;
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, tables, &[planned.globals]);
                pass.set_bind_group(1, &bind, &[*params]);
                Some(sweep(pass, run, once))
            }
        }
    }

    /// Issues one run of instances of the display list over `run`.
    ///
    /// The pipeline and the bindings are set once. What each rectangle carries of its own is where
    /// its instances start in the frame's order list and how many there are, and that reaches the
    /// draw as a vertex buffer offset — asking for a non-zero base instance instead would need
    /// OpenGL 4.2, and the oldest device this runs on is 3.3. A rectangle no instance reaches
    /// issues nothing.
    #[allow(
        clippy::too_many_arguments,
        reason = "every one of them is a property of the one draw being issued"
    )]
    fn instances(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        planned: &PlannedPass,
        tables: Option<&wgpu::BindGroup>,
        kind: PipelineKind,
        texture: Option<u32>,
        format: wgpu::TextureFormat,
        run: &[Swept<'_>],
    ) -> Option<u32> {
        let tables = tables?;
        let lane = crate::renderer::frame::FrameBuffers::lane(kind)?;
        let instances =
            self.buffers
                .instance_bind_group(self.gpu, self.pipelines.layouts(), lane)?;
        let atlas = match texture {
            None => None,
            // A sprite whose atlas texture was never created cannot be drawn at all: it happens
            // when a device was rebuilt and the content has not been rasterised again yet, and
            // drawing it against another texture would show a stranger's pixels.
            Some(texture) => Some(self.atlas.bind_group(decode_texture(texture))?),
        };
        let pipeline = self.pipelines.get(self.gpu, kind, format)?;
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, tables, &[planned.globals]);
        pass.set_bind_group(1, &instances, &[]);
        if let Some(bind_group) = atlas {
            pass.set_bind_group(2, bind_group, &[]);
        }

        let mut issued = 0;
        for (scissor, draw) in run {
            let PlannedDraw::Instances { first, count, .. } = draw else {
                continue;
            };
            if *count == 0 {
                continue;
            }
            let offset = u64::from(*first) * size_of::<crate::buffer::persist::OrderEntry>() as u64;
            pass.set_vertex_buffer(0, self.buffers.orders.buffer().slice(offset..));
            scissor_to(pass, scissor);
            pass.draw(0..4, 0..*count);
            issued += 1;
        }
        Some(issued)
    }

    /// Issues one run of an application's own primitive effect over `run`.
    ///
    /// The shaded lane is drawn exactly as [`Recorder::instances`] draws the framework's own — one
    /// order list swept across each rectangle's scissor — through a per-application pipeline and
    /// with a parameter block of its own beside the frame's tables. An effect the renderer was
    /// never told about draws nothing rather than the rectangle drawn by whatever pipeline happened
    /// to be bound.
    #[allow(
        clippy::too_many_arguments,
        reason = "every one of them is a property of the one draw being issued"
    )]
    fn shaded(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        planned: &PlannedPass,
        tables: Option<&wgpu::BindGroup>,
        shader: zgui_scene::ShaderId,
        params: zgui_scene::ShaderParamsSlot,
        format: wgpu::TextureFormat,
        run: &[Swept<'_>],
    ) -> Option<u32> {
        let Some(tables) = tables else {
            self.pipelines
                .note_undrawable_effect(shader, "the frame's side tables were not bound");
            return None;
        };
        let Some(block_offset) = self.buffers.effect_offset(params) else {
            self.pipelines
                .note_undrawable_effect(shader, "this frame staged no parameters for its block");
            return None;
        };
        let Some(block) = self
            .buffers
            .effect_bind_group(self.gpu, self.pipelines.layouts())
        else {
            self.pipelines
                .note_undrawable_effect(shader, "the parameter buffer was never uploaded");
            return None;
        };
        let lane = crate::renderer::frame::FrameBuffers::SHADED_LANE;
        let Some(instances) =
            self.buffers
                .instance_bind_group(self.gpu, self.pipelines.layouts(), lane)
        else {
            self.pipelines
                .note_undrawable_effect(shader, "the instance arena was not bound");
            return None;
        };
        let Some(pipeline) = self.pipelines.effect(self.gpu, shader, format) else {
            self.pipelines.note_undrawable_effect(
                shader,
                "no pipeline: the effect is not registered on this device, or would not build",
            );
            return None;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, tables, &[planned.globals]);
        pass.set_bind_group(1, &instances, &[]);
        pass.set_bind_group(2, &block, &[block_offset]);

        let mut issued = 0;
        for (scissor, draw) in run {
            let PlannedDraw::Shaded { first, count, .. } = draw else {
                continue;
            };
            if *count == 0 {
                continue;
            }
            let offset = u64::from(*first) * size_of::<crate::buffer::persist::OrderEntry>() as u64;
            pass.set_vertex_buffer(0, self.buffers.orders.buffer().slice(offset..));
            scissor_to(pass, scissor);
            pass.draw(0..4, 0..*count);
            issued += 1;
        }
        Some(issued)
    }

    /// Issues one filtering pass of an application's own shader over `run`.
    ///
    /// A filter covers its region and reads a texture, so it binds the block describing what it
    /// reads, the source and the sampler, and the effect's own parameters beside them — and none of
    /// the frame's tables: it is cut to its region by the scissor rather than clipped per fragment,
    /// so it reads no clip chain. One draw per rectangle of the run.
    #[allow(
        clippy::too_many_arguments,
        reason = "every one of them is a property of the one draw being issued"
    )]
    fn effect_filter(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        source: TargetRef,
        shader: zgui_scene::ShaderId,
        params: u32,
        block: u32,
        format: wgpu::TextureFormat,
        run: &[Swept<'_>],
    ) -> Option<u32> {
        let view = self.view(source)?;
        let read = self.buffers.filtered_bind_group(
            self.gpu,
            self.pipelines.layouts(),
            view,
            self.sampler,
        )?;
        let own = self
            .buffers
            .effect_bind_group(self.gpu, self.pipelines.layouts())?;
        let pipeline = self.pipelines.effect(self.gpu, shader, format)?;
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &read, &[params]);
        pass.set_bind_group(1, &own, &[block]);
        Some(sweep(pass, run, once))
    }

    /// Issues one draw that reads a target through a block of its own, once per rectangle.
    fn textured(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        kind: PipelineKind,
        source: TargetRef,
        params: u32,
        tables: Option<(&wgpu::BindGroup, u32)>,
        format: wgpu::TextureFormat,
        run: &[Swept<'_>],
    ) -> Option<u32> {
        let view = self.view(source)?;
        let bind = self.buffers.filtered_bind_group(
            self.gpu,
            self.pipelines.layouts(),
            view,
            self.sampler,
        )?;
        let pipeline = self.pipelines.get(self.gpu, kind, format)?;
        pass.set_pipeline(pipeline);
        let group = match tables {
            Some((bind_group, offset)) => {
                pass.set_bind_group(0, bind_group, &[offset]);
                1
            }
            None => 0,
        };
        pass.set_bind_group(group, &bind, &[params]);
        Some(sweep(pass, run, once))
    }

    /// A view of a target.
    fn view(&self, target: TargetRef) -> Option<&wgpu::TextureView> {
        match target {
            TargetRef::Composed => Some(self.composed.view()),
            TargetRef::Pool(slot) => Some(self.pool.view(slot)),
        }
    }

    /// A target's texture.
    fn texture(&self, target: TargetRef) -> Option<&wgpu::Texture> {
        match target {
            TargetRef::Composed => Some(self.composed.texture()),
            TargetRef::Pool(slot) => Some(self.pool.texture(slot)),
        }
    }

    /// A target's format.
    fn format(&self, target: TargetRef) -> Option<wgpu::TextureFormat> {
        match target {
            TargetRef::Composed => Some(self.composed.format()),
            TargetRef::Pool(slot) => Some(slot.format()),
        }
    }

    /// A target's extent in texels.
    fn extent(&self, target: TargetRef) -> zgui_geom::Size<i32, Device> {
        match target {
            TargetRef::Composed => self.composed.used().size,
            TargetRef::Pool(slot) => slot.scale().extent(self.pool.region()),
        }
    }
}

/// One rectangle of a swept run: where to scissor, and that rectangle's own version of the draw.
///
/// Every entry of a run is the same kind of draw drawn by the same pipeline. What differs is which
/// instances of the display list reach that rectangle, which is what the culling worked out when
/// the frame was planned.
type Swept<'plan> = (Rect<i32, Device>, &'plan PlannedDraw);

/// Draws a unit quad under each rectangle of `run`, taking each one's instances from `of`.
///
/// The pipeline and its bindings are set by the caller and not touched here: what separates one
/// damage rectangle from the next is the scissor alone, and setting one is a small fraction of
/// what setting the rest costs. Answers how many draws that took, which is one per rectangle that
/// had anything to draw.
fn sweep(
    pass: &mut wgpu::RenderPass<'_>,
    run: &[Swept<'_>],
    of: fn(&PlannedDraw) -> core::ops::Range<u32>,
) -> u32 {
    let mut issued = 0;
    for (scissor, draw) in run {
        let instances = of(draw);
        if instances.is_empty() {
            continue;
        }
        scissor_to(pass, scissor);
        pass.draw(0..4, instances);
        issued += 1;
    }
    issued
}

/// The one instance every draw that is not a run of the display list draws.
fn once(_: &PlannedDraw) -> core::ops::Range<u32> {
    0..1
}

/// A vector composite's own instances, which a rectangle carries for itself.
fn composited(draw: &PlannedDraw) -> core::ops::Range<u32> {
    match draw {
        PlannedDraw::Vector { first, count, .. } => *first..*first + *count,
        _ => 0..0,
    }
}

/// Cuts the pass to `scissor`, clamped to the target it is drawing into.
fn scissor_to(pass: &mut wgpu::RenderPass<'_>, scissor: &Rect<i32, Device>) {
    pass.set_scissor_rect(
        scissor.origin.x.max(0) as u32,
        scissor.origin.y.max(0) as u32,
        scissor.size.width.max(0) as u32,
        scissor.size.height.max(0) as u32,
    );
}

/// The order of draw kinds a run's rectangles all replay part of, when it is safe to sweep them.
///
/// A damage rectangle is planned as a replay of the batch stream under its own scissor, holding
/// only the kinds some instance of its own reaches. So the rectangles of one run draw
/// **subsequences of one order**, and this answers the longest of them where every other is a
/// subsequence of it. Two things are deliberately not required to match. Which instances reach a
/// rectangle is what the culling worked out and is carried per rectangle; and a rectangle that
/// draws fewer kinds than another simply sits out the draws it has none of.
///
/// Sweeping reorders the draws — every rectangle's first draw, then every rectangle's second — and
/// that is the same picture only while no pixel lies under two scissors. It usually does not: a
/// [`DamageSet`](zgui_bits::DamageSet) holds pairwise disjoint rectangles by construction. A
/// backdrop widens the set afterwards and can put one rectangle inside another, which is the case
/// this returns `None` for.
fn shared_shape<'plan>(
    plan: &'plan FramePlan,
    passes: &[&PlannedPass],
    scissors: &[Rect<i32, Device>],
) -> Option<&'plan [PlannedDraw]> {
    let template = passes
        .iter()
        .map(|held| plan.draws_of(held))
        .max_by_key(|draws| draws.len())?;
    if passes.len() == 1 {
        return Some(template);
    }
    // The block describing the target is bound once for the whole run, so a rectangle wanting a
    // different one has to be drawn on its own.
    if !passes[1..]
        .iter()
        .all(|held| held.globals == passes[0].globals)
    {
        return None;
    }
    let subsequences = passes
        .iter()
        .all(|held| subsequence(plan.draws_of(held), template));
    if !subsequences {
        return None;
    }
    let disjoint = scissors.iter().enumerate().all(|(index, rect)| {
        !scissors[index + 1..]
            .iter()
            .any(|other| other.intersects(*rect))
    });
    disjoint.then_some(template)
}

/// Whether `draws` appears inside `template` in order, matching on shape.
fn subsequence(draws: &[PlannedDraw], template: &[PlannedDraw]) -> bool {
    let mut wanted = draws.iter();
    let mut next = wanted.next();
    for shape in template {
        if next.is_some_and(|draw| same_shape(draw, shape)) {
            next = wanted.next();
        }
    }
    next.is_none()
}

/// Whether two draws are the same kind drawn by the same pipeline with the same bindings.
///
/// Which instances they draw is left out, and for the two that carry a run of the order list that
/// is the only thing allowed to differ. Everything else has to match exactly, because everything
/// else is state that is set once for the whole sweep.
fn same_shape(a: &PlannedDraw, b: &PlannedDraw) -> bool {
    match (a, b) {
        (
            PlannedDraw::Instances { kind, texture, .. },
            PlannedDraw::Instances {
                kind: other,
                texture: from,
                ..
            },
        ) => kind == other && texture == from,
        (
            PlannedDraw::Shaded { shader, params, .. },
            PlannedDraw::Shaded {
                shader: other,
                params: from,
                ..
            },
        ) => shader == other && params == from,
        (PlannedDraw::Vector { target, .. }, PlannedDraw::Vector { target: other, .. }) => {
            target == other
        }
        _ => a == b,
    }
}

/// A device-pixel rectangle in a target's own texels, cut to its extent.
fn scaled(
    rect: Rect<i32, Device>,
    target: TargetRef,
    extent: zgui_geom::Size<i32, Device>,
) -> Rect<i32, Device> {
    let scale = target.scale();
    let scaled = Rect::from_corners(
        zgui_geom::Point::new(scale.texel(rect.left()), scale.texel(rect.top())),
        zgui_geom::Point::new(
            scale.texel(rect.right().max(rect.left())),
            scale.texel(rect.bottom().max(rect.top())),
        ),
    );
    scaled
        .intersection(Rect::new(zgui_geom::Point::new(0, 0), extent))
        .unwrap_or(Rect::ZERO)
}

/// `region` cut to what both textures actually hold.
fn clamp(region: Rect<i32, Device>, from: &wgpu::Texture, to: &wgpu::Texture) -> Rect<i32, Device> {
    let bound = |texture: &wgpu::Texture| {
        Rect::new(
            zgui_geom::Point::new(0, 0),
            zgui_geom::Size::new(texture.width() as i32, texture.height() as i32),
        )
    };
    region
        .intersection(bound(from))
        .and_then(|clipped| clipped.intersection(bound(to)))
        .unwrap_or(Rect::ZERO)
}

/// One corner of a copy between two textures.
fn texel_copy(
    texture: &wgpu::Texture,
    region: Rect<i32, Device>,
) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d {
            x: region.origin.x.max(0) as u32,
            y: region.origin.y.max(0) as u32,
            z: 0,
        },
        aspect: wgpu::TextureAspect::All,
    }
}

/// The atlas texture a batch's packed identifier names.
fn decode_texture(packed: u32) -> zgui_atlas::TextureId {
    let kind = match packed >> 16 {
        0 => zgui_atlas::TextureKind::Mono,
        1 => zgui_atlas::TextureKind::Subpixel,
        2 => zgui_atlas::TextureKind::Color,
        _ => zgui_atlas::TextureKind::Image,
    };
    zgui_atlas::TextureId::new(kind, packed & 0xffff)
}

/// Whether `next` can be issued inside the pass `first` opens.
///
/// It can when the two write the same attachment under the same globals and `next` keeps what is
/// there. A pass that discards cannot join one: its clear covers the whole attachment whatever the
/// scissor says, so it would throw away everything drawn before it in that pass.
fn shares_a_pass(first: &PlannedPass, next: &PlannedPass) -> bool {
    first.target == next.target
        && first.globals == next.globals
        && matches!(next.load, PassLoad::Keep)
}

#[cfg(test)]
mod tests {
    use super::decode_texture;
    use zgui_atlas::{AtlasTile, TextureId, TextureKind, TileId};
    use zgui_geom::{Point, Rect, Size};
    use zgui_scene::SpriteTile;

    #[test]
    fn a_packed_texture_identifier_round_trips_through_a_batch() {
        // The display list packs the pool and the index into one number so that a batch can break
        // on a change of texture; this is the other end of that packing, and the two agreeing is
        // what stops a colour sprite being drawn against the coverage pool.
        for kind in TextureKind::ALL {
            for index in [0, 1, 7] {
                let tile = AtlasTile {
                    texture: TextureId::new(kind, index),
                    tile: TileId(0),
                    bounds: Rect::new(Point::new(0, 0), Size::new(1, 1)),
                };
                let packed = SpriteTile::of(tile).texture;
                assert_eq!(decode_texture(packed), TextureId::new(kind, index));
            }
        }
    }
}
