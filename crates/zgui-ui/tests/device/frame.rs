//! One frame of the application, and what the display list said would be in it.

use zgui::geom::{Device, DevicePx, Point, Rect, Size};
use zgui::render::FrameOutcome;
use zgui::scene::Scene;
use zgui_render_wgpu::{Pixels, WgpuRenderer};

/// Which primitive array a drawing arrived in.
///
/// The display list picks a route per shape. A shape it recognises arrives as quads or as one mark
/// item, a small one as a single-channel sprite, and only what is left as a vector item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A vector item, for the general rasteriser.
    Vector,
    /// A colour sprite, from the CPU layer route.
    Colour,
    /// A mark item: discs, boxes, polylines or path glyphs.
    Mark,
    /// A single-channel sprite: a shape rasterised as a coverage mask, or a glyph.
    Mask,
    /// A quad: a shape the analytic route recognised, or a box's background and border.
    Quad,
}

/// One drawing the display list held, and where it claimed its ink would land.
#[derive(Clone, Copy, Debug)]
pub struct Drawing {
    /// Its index in the array of its kind.
    pub order: usize,
    /// The array it is in.
    pub kind: Kind,
    /// What it paints, in device pixels, as the display list measured it.
    pub ink: Rect<DevicePx, Device>,
    /// The clip chain it draws through.
    pub clip: u32,
    /// Whether it is filled, and whether it is stroked.
    pub painted: (bool, bool),
}

/// One filled rectangle the display list held.
///
/// The caret is one of these, and so is a border and a background — which is why the rectangle is
/// kept rather than a count: telling a caret from a field's own edge is a question about where and
/// how wide, and a fixture that counted quads would answer it for neither.
///
/// The rectangle is where the ink *lands*, which for a box under a transform is not the rectangle
/// the display list recorded. See [`placed`].
#[derive(Clone, Copy, Debug)]
pub struct Filled {
    /// Its place in the painting order.
    pub order: usize,
    /// Where it lands, in device pixels.
    pub bounds: Rect<DevicePx, Device>,
}

/// One glyph the display list held.
///
/// The tile is what makes this an assertion about *which* letters were drawn. A glyph is rasterised
/// once per face, size and glyph index and cached under that key, so two runs of the same string in
/// the same style read the same tiles in the same order — and two different strings do not.
///
/// The bounds are where the ink *lands*, which for text under a transform is not the rectangle the
/// display list recorded. See [`placed`].
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    /// Where it lands, in device pixels.
    pub bounds: Rect<DevicePx, Device>,
    /// Which texture of which pool holds its coverage, and which allocation in it.
    pub tile: (u32, u32),
}

/// What one of the application's frames produced.
pub struct Frame {
    /// The composed target, read back off the device.
    pub pixels: Pixels,
    /// Every primitive only a drawing produces: each vector item, then each colour sprite, then
    /// each mark item.
    pub drawings: Vec<Drawing>,
    /// Every primitive a drawing can arrive as: the drawings, then each quad and each
    /// single-channel sprite. See [`Frame::shapes_in`].
    pub shapes: Vec<Drawing>,
    /// Every filled rectangle it held, in painting order.
    pub quads: Vec<Filled>,
    /// Every glyph it held, in painting order.
    pub glyphs: Vec<Glyph>,
    /// The persistent target frames are composed into, which is what a surface shows.
    pub composed: Pixels,
    /// How many rasterisation passes the frame planned.
    pub passes: usize,
    /// How many vector items the damage cull dropped.
    pub culled: usize,
    /// How many passes the device reported having rasterised.
    pub rasterised: u32,
    /// Each pass's region, its item range and whether it composites per item.
    pub pass_regions: Vec<(Rect<i32, Device>, core::ops::Range<usize>, bool)>,
}

impl Frame {
    /// Reads back what `renderer` just drew, beside what `scene` said would be in it.
    pub fn record(renderer: &mut WgpuRenderer, scene: &Scene, outcome: &FrameOutcome) -> Self {
        let plan = scene.pass_plan();
        let mut drawings: Vec<Drawing> = scene
            .primitives
            .vectors
            .iter()
            .enumerate()
            .map(|(order, item)| Drawing {
                order,
                kind: Kind::Vector,
                ink: item.ink,
                clip: item.clip.0,
                painted: (item.fill.is_some(), item.stroke.is_some()),
            })
            .collect();
        // A drawing on the CPU layer route arrives as one colour sprite.
        drawings.extend(scene.primitives.color_sprites.iter().enumerate().map(
            |(order, sprite)| Drawing {
                order,
                kind: Kind::Colour,
                ink: placed(scene, sprite.bounds, sprite.transform),
                clip: sprite.clip,
                painted: (true, false),
            },
        ));
        // A shape the marks route recognised arrives as one mark item.
        drawings.extend(
            scene
                .primitives
                .marks
                .iter()
                .enumerate()
                .map(|(order, mark)| Drawing {
                    order,
                    kind: Kind::Mark,
                    ink: placed(scene, mark.bounds, mark.transform),
                    clip: mark.clip,
                    painted: (true, false),
                }),
        );
        let mut shapes = drawings.clone();
        shapes.extend(
            scene
                .primitives
                .quads
                .iter()
                .enumerate()
                .map(|(order, quad)| Drawing {
                    order,
                    kind: Kind::Quad,
                    ink: placed(scene, quad.bounds, quad.transform),
                    clip: quad.clip,
                    painted: (!quad.fill.is_none(), !quad.stroke.is_none()),
                }),
        );
        shapes.extend(
            scene
                .primitives
                .mono_sprites
                .iter()
                .enumerate()
                .map(|(order, sprite)| Drawing {
                    order,
                    kind: Kind::Mask,
                    ink: placed(scene, sprite.bounds, sprite.transform),
                    clip: sprite.clip,
                    painted: (true, false),
                }),
        );
        let quads = scene
            .primitives
            .quads
            .iter()
            .enumerate()
            .map(|(order, quad)| Filled {
                order,
                bounds: placed(scene, quad.bounds, quad.transform),
            })
            .collect();
        // All three sprite arrays, because which one a glyph lands in is the renderer's decision
        // about coverage and not a property of the text: a fixture that read only the monochrome
        // ones would see no letters at all on a machine that shapes them subpixel-antialiased.
        let mut glyphs: Vec<Glyph> = Vec::new();
        glyphs.extend(scene.primitives.mono_sprites.iter().map(|sprite| Glyph {
            bounds: placed(scene, sprite.bounds, sprite.transform),
            tile: (sprite.tile.texture, sprite.tile.tile),
        }));
        glyphs.extend(
            scene
                .primitives
                .subpixel_sprites
                .iter()
                .map(|sprite| Glyph {
                    bounds: placed(scene, sprite.bounds, sprite.transform),
                    tile: (sprite.tile.texture, sprite.tile.tile),
                }),
        );
        glyphs.extend(scene.primitives.color_sprites.iter().map(|sprite| Glyph {
            bounds: placed(scene, sprite.bounds, sprite.transform),
            tile: (sprite.tile.texture, sprite.tile.tile),
        }));

        Self {
            pixels: renderer
                .read_presented()
                .expect("these fixtures draw to a texture, which can be read back"),
            composed: renderer.read_composed(),
            drawings,
            shapes,
            quads,
            glyphs,
            pass_regions: plan
                .passes
                .iter()
                .map(|pass| (pass.region, pass.items.clone(), pass.instanced))
                .collect(),
            passes: plan.passes.len(),
            culled: plan.culled,
            rasterised: outcome.stats().map_or(0, |stats| stats.vector_passes),
        }
    }

    /// Every shape whose middle is inside `rect`, without the quads that fill one of `boxes`.
    ///
    /// `boxes` are the element boxes of the document. A background or a border is a quad the size
    /// of its element's box, and a shape the analytic route drew is the size of the shape. A glyph
    /// that arrives as a single-channel sprite is still in the result, so a caller asks inside a
    /// rectangle with no text, or removes the text itself.
    pub fn shapes_in(
        &self,
        rect: Rect<DevicePx, Device>,
        boxes: &[Rect<DevicePx, Device>],
    ) -> Vec<Drawing> {
        self.shapes
            .iter()
            .copied()
            .filter(|shape| middle_inside(rect, shape.ink))
            .filter(|shape| {
                shape.kind != Kind::Quad || !boxes.iter().any(|held| same(*held, shape.ink))
            })
            .collect()
    }
}

/// Whether the middle of `inner` is inside `outer`.
fn middle_inside(outer: Rect<DevicePx, Device>, inner: Rect<DevicePx, Device>) -> bool {
    let x = inner.origin.x.0 + inner.size.width.0 / 2.0;
    let y = inner.origin.y.0 + inner.size.height.0 / 2.0;
    x >= outer.origin.x.0
        && x <= outer.origin.x.0 + outer.size.width.0
        && y >= outer.origin.y.0
        && y <= outer.origin.y.0 + outer.size.height.0
}

/// Whether two rectangles are the same to half a pixel on every edge.
fn same(left: Rect<DevicePx, Device>, right: Rect<DevicePx, Device>) -> bool {
    const SLACK: f32 = 0.5;
    (left.origin.x.0 - right.origin.x.0).abs() <= SLACK
        && (left.origin.y.0 - right.origin.y.0).abs() <= SLACK
        && (left.size.width.0 - right.size.width.0).abs() <= SLACK
        && (left.size.height.0 - right.size.height.0).abs() <= SLACK
}

/// Where `bounds` lands once the transform it draws under has been applied.
///
/// A transform is applied when a box is *drawn*, not when it is laid out: the rectangle the display
/// list records is the one the box would have had untransformed, and the primitive carries the
/// matrix it reaches the screen through. A fixture that read the recorded rectangle would report a
/// translated strip as sitting exactly where it started — which is the one shape in which a
/// carousel that never moves and one that moves correctly are the same reading.
///
/// The four corners are put through the matrix and the box around them is taken, so a rotation
/// answers with the rectangle its ink actually covers.
fn placed(scene: &Scene, bounds: [f32; 4], transform: u32) -> Rect<DevicePx, Device> {
    let Some(matrix) = scene.spatial.resolve_at(transform) else {
        return rect_of(bounds);
    };
    let [x, y, width, height] = bounds;
    let corners = [
        matrix.transform_point(x, y, 0.0),
        matrix.transform_point(x + width, y, 0.0),
        matrix.transform_point(x, y + height, 0.0),
        matrix.transform_point(x + width, y + height, 0.0),
    ];
    let left = corners
        .iter()
        .map(|point| point[0])
        .fold(f32::MAX, f32::min);
    let right = corners
        .iter()
        .map(|point| point[0])
        .fold(f32::MIN, f32::max);
    let top = corners
        .iter()
        .map(|point| point[1])
        .fold(f32::MAX, f32::min);
    let bottom = corners
        .iter()
        .map(|point| point[1])
        .fold(f32::MIN, f32::max);
    rect_of([left, top, right - left, bottom - top])
}

/// One `[x, y, width, height]` as a rectangle.
fn rect_of(bounds: [f32; 4]) -> Rect<DevicePx, Device> {
    Rect::new(
        Point::new(DevicePx(bounds[0]), DevicePx(bounds[1])),
        Size::new(DevicePx(bounds[2]), DevicePx(bounds[3])),
    )
}
