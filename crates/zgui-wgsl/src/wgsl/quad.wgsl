// DERIVED-FROM: the GPUI project, crates/gpui_wgpu/src/shaders.wgsl (Apache-2.0)
// The quad fragment shader — the inner-edge signed distance, the border blend, and the dash
// parameterisation that lays dashes out clockwise around the whole perimeter — is adapted from
// that work, which is licensed under the Apache License, Version 2.0, and has been modified: every
// corner radius is a pair of elliptical semi-axes rather than a scalar, so each straight side takes
// the radii of its own axis, each quarter corner's arc length is a Ramanujan quarter-ellipse
// perimeter rather than `r * pi / 2`, and the position along a corner uses the eccentric anomaly.
// The background is a paint-table reference rather than an inline two-stop gradient, the clip is a
// chain evaluated by a shared coverage function rather than four interpolated distances, and dotted
// borders are a second style rather than an unimplemented one.

// One paint per border side, in the order the widths are given.
struct Strokes {
    top: PaintRef,
    right: PaintRef,
    bottom: PaintRef,
    left: PaintRef,
}

struct Quad {
    order: u32,
    style: u32,
    bounds: Bounds,
    radii: Radii,
    border: Edges,
    fill: PaintRef,
    strokes: Strokes,
    clip: u32,
    transform: u32,
    // The superellipse exponent the corners are cut with; two is the ellipse a corner radius has
    // always drawn.
    shape: f32,
    // Where the space the two paints were resolved in has its origin, subtracted from the sample
    // point before either is evaluated. Zero for a quad drawn where its paints were resolved.
    paint_origin: Vector2,
}

@group(1) @binding(0) var quads: texture_2d<u32>;

/// One quad, which spans 7 texels of the arena.
fn load_quad(slot: u32) -> Quad {
    let base = slot * 9u;
    let t0 = textureLoad(quads, table_texel(base + 0u), 0);
    let t1 = textureLoad(quads, table_texel(base + 1u), 0);
    let t2 = textureLoad(quads, table_texel(base + 2u), 0);
    let t3 = textureLoad(quads, table_texel(base + 3u), 0);
    let t4 = textureLoad(quads, table_texel(base + 4u), 0);
    let t5 = textureLoad(quads, table_texel(base + 5u), 0);
    let t6 = textureLoad(quads, table_texel(base + 6u), 0);
    let t7 = textureLoad(quads, table_texel(base + 7u), 0);
    let t8 = textureLoad(quads, table_texel(base + 8u), 0);
    return Quad(
        t0.x,
        t0.y,
        Bounds(bitcast<f32>(t0.z), bitcast<f32>(t0.w), bitcast<f32>(t1.x), bitcast<f32>(t1.y)),
        Radii(bitcast<f32>(t1.z), bitcast<f32>(t1.w), bitcast<f32>(t2.x), bitcast<f32>(t2.y), bitcast<f32>(t2.z), bitcast<f32>(t2.w), bitcast<f32>(t3.x), bitcast<f32>(t3.y)),
        Edges(bitcast<f32>(t3.z), bitcast<f32>(t3.w), bitcast<f32>(t4.x), bitcast<f32>(t4.y)),
        PaintRef(t4.z, t4.w),
        Strokes(
            PaintRef(t5.x, t5.y),
            PaintRef(t5.z, t5.w),
            PaintRef(t6.x, t6.y),
            PaintRef(t6.z, t6.w),
        ),
        t7.x,
        t7.y,
        bitcast<f32>(t7.z),
        Vector2(bitcast<f32>(t7.w), bitcast<f32>(t8.x)),
    );
}


// Which side owns the pixel at `corner_to_point`, as an index into `Strokes`.
//
// Where two sides meet they are divided by the diagonal running from the outer corner to the inner
// one, which is the join CSS specifies and the one every engine draws. The test is which side's
// edge is nearest *in units of that side's own width*: halfway along the diagonal the two distances
// are equal fractions, and either answer paints the same colour when the two sides share one.
//
// On a box rounded to a full circle the four diagonals fall on 45 degrees, so each side owns a
// quarter of the ring — which is exactly the arc a spinner leaves out.
fn owning_side(center_to_point: vec2<f32>, half: vec2<f32>, border: Edges) -> u32 {
    // Distance inwards from each outer edge.
    let from_top = center_to_point.y + half.y;
    let from_bottom = half.y - center_to_point.y;
    let from_left = center_to_point.x + half.x;
    let from_right = half.x - center_to_point.x;
    // In units of the side's own width. A side of no width can never be nearest.
    let vertical = select(
        from_top / border.top,
        from_bottom / border.bottom,
        center_to_point.y > 0.0,
    );
    let horizontal = select(
        from_left / border.left,
        from_right / border.right,
        center_to_point.x > 0.0,
    );
    let width_vertical = select(border.top, border.bottom, center_to_point.y > 0.0);
    let width_horizontal = select(border.left, border.right, center_to_point.x > 0.0);
    if width_horizontal <= 0.0 || (width_vertical > 0.0 && vertical <= horizontal) {
        return select(0u, 2u, center_to_point.y > 0.0);
    }
    return select(3u, 1u, center_to_point.x > 0.0);
}

// The paint the owning side is drawn with.
fn stroke_of(strokes: Strokes, side: u32) -> PaintRef {
    if side == 0u {
        return strokes.top;
    }
    if side == 1u {
        return strokes.right;
    }
    if side == 2u {
        return strokes.bottom;
    }
    return strokes.left;
}

const BORDER_SOLID: u32 = 0u;
const BORDER_DASHED: u32 = 1u;
const BORDER_DOTTED: u32 = 2u;

// The record carried across, rather than fetched again on every fragment.
//
// A device with no storage buffers reads these tables out of textures, so the seven texels a quad
// occupies are seven `textureLoad`s **per fragment** — for a record that is the same at every
// fragment of the primitive. The vertex stage has already read it, and what the fragment stage
// needs of it is twenty-four scalars, which is six slots. Even the narrowest device this pipeline
// runs on has fifteen.
struct QuadVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) bounds: vec4<f32>,
    @location(2) @interpolate(flat) radii_low: vec4<f32>,
    @location(3) @interpolate(flat) radii_high: vec4<f32>,
    @location(4) @interpolate(flat) border: vec4<f32>,
    @location(5) @interpolate(flat) paint_origin: vec2<f32>,
    // style, clip, how many rounded tests the clip has, and the two paint references — all whole
    // numbers, so they travel as such.
    @location(6) @interpolate(flat) style_clip: vec4<u32>,
    @location(7) @interpolate(flat) paints: vec4<u32>,
    // The four border paints, two to a slot, in top, right, bottom, left order. Carried for the
    // same reason the fill is: they belong to the primitive, and the fragment stage would otherwise
    // read the quad's own texels again to reach whichever side owns the pixel.
    @location(12) @interpolate(flat) strokes_near: vec4<u32>,
    @location(13) @interpolate(flat) strokes_far: vec4<u32>,
    // The clip's own box, and the fill's colour where the fill is one colour. Both are properties
    // of the primitive rather than of the pixel, and both were being fetched per fragment: the box
    // to reject a fragment outside the clip, the colour to shade every fragment inside it. Carrying
    // them leaves the common case — an unclipped rectangle of one colour — reading no table at all.
    @location(8) @interpolate(flat) clip_box: vec4<f32>,
    @location(9) @interpolate(flat) fill_color: vec4<f32>,
    @location(10) @interpolate(flat) shift: vec2<f32>,
    @location(11) @interpolate(flat) shape: f32,
}

@vertex
fn vs_quad(
    @builtin(vertex_index) vertex: u32,
    @location(0) slot: u32,
    @location(1) shift: vec2<f32>,
) -> QuadVarying {
    let quad = load_quad(slot);
    let local = inflated_corner(vertex, quad.bounds) + shift;
    var out: QuadVarying;
    out.position = to_clip_position(local, quad.transform);
    out.local = local;
    out.bounds = vec4<f32>(quad.bounds.x, quad.bounds.y, quad.bounds.w, quad.bounds.h);
    out.radii_low = vec4<f32>(
        quad.radii.tl_x, quad.radii.tl_y, quad.radii.tr_x, quad.radii.tr_y,
    );
    out.radii_high = vec4<f32>(
        quad.radii.br_x, quad.radii.br_y, quad.radii.bl_x, quad.radii.bl_y,
    );
    out.border = vec4<f32>(
        quad.border.top, quad.border.right, quad.border.bottom, quad.border.left,
    );
    out.paint_origin = vec2<f32>(quad.paint_origin.x, quad.paint_origin.y);
    out.paints = vec4<u32>(
        quad.fill.kind, quad.fill.index, quad.strokes.top.kind, quad.strokes.top.index,
    );
    out.strokes_near = vec4<u32>(
        quad.strokes.top.kind, quad.strokes.top.index,
        quad.strokes.right.kind, quad.strokes.right.index,
    );
    out.strokes_far = vec4<u32>(
        quad.strokes.bottom.kind, quad.strokes.bottom.index,
        quad.strokes.left.kind, quad.strokes.left.index,
    );
    let box = bitcast<vec4<f32>>(textureLoad(clips, table_texel(quad.clip * 11u + 0u), 0));
    let rounded = textureLoad(clips, table_texel(quad.clip * 11u + 9u), 0).x;
    out.clip_box = box;
    out.style_clip = vec4<u32>(quad.style, quad.clip, rounded, 0u);
    out.fill_color = select(
        vec4<f32>(0.0),
        bitcast<vec4<f32>>(textureLoad(paints, table_texel(quad.fill.index * 4u + 2u), 0)),
        quad.fill.kind == PAINT_SOLID,
    );
    out.shift = shift;
    out.shape = quad.shape;
    return out;
}

@fragment
fn fs_quad(in: QuadVarying) -> @location(0) vec4<f32> {
    // Rebuilt from what was carried across, so that nothing below has to change shape.
    let quad = Quad(
        0u,
        in.style_clip.x,
        Bounds(in.bounds.x, in.bounds.y, in.bounds.z, in.bounds.w),
        Radii(
            in.radii_low.x, in.radii_low.y, in.radii_low.z, in.radii_low.w,
            in.radii_high.x, in.radii_high.y, in.radii_high.z, in.radii_high.w,
        ),
        Edges(in.border.x, in.border.y, in.border.z, in.border.w),
        PaintRef(in.paints.x, in.paints.y),
        Strokes(
            PaintRef(in.strokes_near.x, in.strokes_near.y),
            PaintRef(in.strokes_near.z, in.strokes_near.w),
            PaintRef(in.strokes_far.x, in.strokes_far.y),
            PaintRef(in.strokes_far.z, in.strokes_far.w),
        ),
        in.style_clip.y,
        0u,
        in.shape,
        Vector2(in.paint_origin.x, in.paint_origin.y),
    );
    // The clip is in device space, so it is evaluated at the real pixel; the shape is in the
    // primitive's own space, so it is evaluated at the point that maps to this pixel. The box and
    // the count came across from the vertex stage; only a clip that has rounded tests reads the
    // table here, and nearly none has.
    let at = device_position(in.position.xy);
    let box = in.clip_box;
    if at.x < box.x || at.y < box.y || at.x > box.x + box.z || at.y > box.y + box.w {
        return vec4<f32>(0.0);
    }
    let clip = clip_rounded_coverage(at, in.style_clip.y, in.style_clip.z);
    if clip <= 0.0 {
        return vec4<f32>(0.0);
    }
    let point = in.local - in.shift;
    let paint_origin = vec2<f32>(quad.paint_origin.x, quad.paint_origin.y);
    let background = select(
        paint_color(quad.fill, point, paint_origin),
        in.fill_color,
        quad.fill.kind == PAINT_SOLID,
    );

    let size = bounds_size(quad.bounds);
    let half_size = size * 0.5;
    let center_to_point = point - (bounds_origin(quad.bounds) + half_size);

    // Half a pixel is the largest distance between a pixel's centre and an edge that covers it.
    let antialias_threshold = 0.5;

    let corner_to_point = abs(center_to_point) - half_size;
    let corner_radii = pick_corner_radii(center_to_point, quad.radii);
    let unrounded = quad.radii.tl_x == 0.0 && quad.radii.tl_y == 0.0
        && quad.radii.tr_x == 0.0 && quad.radii.tr_y == 0.0
        && quad.radii.br_x == 0.0 && quad.radii.br_y == 0.0
        && quad.radii.bl_x == 0.0 && quad.radii.bl_y == 0.0;
    let no_border = quad.border.top == 0.0 && quad.border.right == 0.0
        && quad.border.bottom == 0.0 && quad.border.left == 0.0;

    if unrounded && no_border {
        // Still antialiased, and it matters more than it looks: every corner of the quad is
        // expanded by a pixel so that a partly covered edge has somewhere to land, so returning
        // the background unweighted here would paint a full-intensity ring one pixel outside every
        // plain rectangle in the frame.
        let square_sdf = max(corner_to_point.x, corner_to_point.y);
        return background * saturate(antialias_threshold - square_sdf) * clip;
    }

    // The widths of the two nearest sides.
    let border = vec2<f32>(
        select(quad.border.right, quad.border.left, center_to_point.x < 0.0),
        select(quad.border.bottom, quad.border.top, center_to_point.y < 0.0),
    );
    // A zero-width side is pushed outside the antialiasing band so that no partial pixel is drawn
    // for a border that is not there.
    let reduced_border = vec2<f32>(
        select(border.x, -antialias_threshold, border.x == 0.0),
        select(border.y, -antialias_threshold, border.y == 0.0),
    );

    let corner_center_to_point = corner_to_point + corner_radii;
    let is_near_rounded_corner = corner_center_to_point.x >= 0.0 && corner_center_to_point.y >= 0.0;
    let straight_inner_corner_to_point = corner_to_point + reduced_border;
    let is_beyond_inner_straight_border = straight_inner_corner_to_point.x > 0.0
        || straight_inner_corner_to_point.y > 0.0;
    let is_within_inner_straight_border = straight_inner_corner_to_point.x < -antialias_threshold
        && straight_inner_corner_to_point.y < -antialias_threshold;

    if is_within_inner_straight_border && !is_near_rounded_corner {
        return background * clip;
    }

    // Positive outside the outer edge of the border, negative inside it.
    let outer_sdf = quad_sdf_impl(corner_center_to_point, corner_radii, quad.shape);

    // Positive inside the inner edge of the border, negative within the border itself.
    var inner_sdf = 0.0;
    if corner_center_to_point.x <= 0.0 || corner_center_to_point.y <= 0.0 {
        inner_sdf = -max(straight_inner_corner_to_point.x, straight_inner_corner_to_point.y);
    } else if is_beyond_inner_straight_border {
        inner_sdf = -1.0;
    } else if quad.shape == CORNER_ROUND
        && reduced_border.x == reduced_border.y
        && corner_radii.x == corner_radii.y
    {
        // Circular inner edge: the outer distance shifted inwards is exact.
        inner_sdf = -(outer_sdf + reduced_border.x);
    } else {
        let ellipse_radii = max(vec2<f32>(0.0), corner_radii - reduced_border);
        inner_sdf = quarter_ellipse_sdf(corner_center_to_point, ellipse_radii, quad.shape);
    }

    let border_sdf = max(inner_sdf, outer_sdf);

    var color = background;
    if border_sdf < antialias_threshold {
        // Which side's colour this pixel takes. A side left unpainted — `border-top-color:
        // transparent`, which is how a spinner is written — draws nothing at all here rather than
        // blending a transparent colour, so the background shows through the gap and the ring can
        // be seen to turn.
        let side = owning_side(center_to_point, half_size, quad.border);
        let stroke = stroke_of(quad.strokes, side);
        if stroke.kind == PAINT_NONE {
            return color * saturate(antialias_threshold - outer_sdf) * clip;
        }
        var border_color = paint_color(stroke, point, paint_origin);
        let style = quad.style & 0xffu;
        if style != BORDER_SOLID {
            border_color *= dash_coverage(
                quad,
                style,
                point,
                center_to_point,
                corner_center_to_point,
                corner_radii,
                is_near_rounded_corner,
                unrounded,
                antialias_threshold,
            );
        }
        let blended = over_premultiplied(background, border_color);
        color = mix(background, blended, saturate(antialias_threshold - inner_sdf));
    }

    return color * saturate(antialias_threshold - outer_sdf) * clip;
}

// Premultiplied source-over.
fn over_premultiplied(below: vec4<f32>, above: vec4<f32>) -> vec4<f32> {
    return above + below * (1.0 - above.a);
}

// Ramanujan's approximation of a quarter-ellipse's arc length, which reduces to `r * pi / 2` when
// the two semi-axes are equal.
fn quarter_ellipse_perimeter(r: vec2<f32>) -> f32 {
    if r.x <= 0.0 && r.y <= 0.0 {
        return 0.0;
    }
    let h = ((r.x - r.y) * (r.x - r.y)) / max((r.x + r.y) * (r.x + r.y), 1e-6);
    return 0.25 * M_PI * (r.x + r.y) * (1.0 + (3.0 * h) / (10.0 + sqrt(max(4.0 - 3.0 * h, 0.0))));
}

// The slower of two dash velocities, so a corner takes the larger dashes of the two sides it
// joins. A zero velocity means a zero-width side, which contributes nothing.
fn corner_dash_velocity(first: f32, second: f32) -> f32 {
    if first == 0.0 {
        return second;
    }
    if second == 0.0 {
        return first;
    }
    return min(first, second);
}

// Coverage of a dash at position `t` in dash space, where one dash period has length one.
fn dash_alpha(t: f32, period: f32, length: f32, velocity: f32, threshold: f32) -> f32 {
    let half_period = period * 0.5;
    let half_length = length * 0.5;
    let centered = sdf_fmod(t + half_period - half_length, period) - half_period;
    let signed_distance = abs(centered) - half_length;
    return saturate(threshold - signed_distance / max(velocity, 1e-6));
}

// How much of this pixel a dashed or dotted border covers.
//
// Dash size is proportional to border width, which is what browsers do and what keeps dashes from
// overlapping where the border is thicker than the dash. A dotted border is the same machinery
// with a one-to-one dash and gap.
fn dash_coverage(
    quad: Quad,
    style: u32,
    point: vec2<f32>,
    center_to_point: vec2<f32>,
    corner_center_to_point: vec2<f32>,
    corner_radii: vec2<f32>,
    is_near_rounded_corner: bool,
    unrounded: bool,
    threshold: f32,
) -> f32 {
    let dash_per_width = select(2.0, 1.0, style == BORDER_DOTTED);
    let gap_per_width = 1.0;
    let period_per_width = dash_per_width + gap_per_width;
    let dv_numerator = 1.0 / period_per_width;

    let size = bounds_size(quad.bounds);
    let origin = bounds_origin(quad.bounds);
    let local = point - origin;

    var t = 0.0;
    var max_t = 0.0;
    var velocity = 0.0;

    if unrounded {
        // Without rounded corners each side lays its dashes out on its own, so every side starts
        // and ends with a dash.
        let is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
        let widths = vec2<f32>(
            max(quad.border.bottom, quad.border.top),
            max(quad.border.right, quad.border.left),
        );
        let width = select(widths.y, widths.x, is_horizontal);
        velocity = select(0.0, dv_numerator / width, width > 0.0);
        t = select(local.y, local.x, is_horizontal) * velocity;
        max_t = select(size.y, size.x, is_horizontal) * velocity;
    } else {
        let r_tl = vec2<f32>(quad.radii.tl_x, quad.radii.tl_y);
        let r_tr = vec2<f32>(quad.radii.tr_x, quad.radii.tr_y);
        let r_br = vec2<f32>(quad.radii.br_x, quad.radii.br_y);
        let r_bl = vec2<f32>(quad.radii.bl_x, quad.radii.bl_y);

        let dv_t = select(0.0, dv_numerator / quad.border.top, quad.border.top > 0.0);
        let dv_r = select(0.0, dv_numerator / quad.border.right, quad.border.right > 0.0);
        let dv_b = select(0.0, dv_numerator / quad.border.bottom, quad.border.bottom > 0.0);
        let dv_l = select(0.0, dv_numerator / quad.border.left, quad.border.left > 0.0);

        // A straight side runs between the two corners on its own axis, so it takes the x radii of
        // the horizontal sides and the y radii of the vertical ones.
        let s_t = max(size.x - r_tl.x - r_tr.x, 0.0) * dv_t;
        let s_r = max(size.y - r_tr.y - r_br.y, 0.0) * dv_r;
        let s_b = max(size.x - r_br.x - r_bl.x, 0.0) * dv_b;
        let s_l = max(size.y - r_bl.y - r_tl.y, 0.0) * dv_l;

        let cv_tr = corner_dash_velocity(dv_t, dv_r);
        let cv_br = corner_dash_velocity(dv_b, dv_r);
        let cv_bl = corner_dash_velocity(dv_b, dv_l);
        let cv_tl = corner_dash_velocity(dv_t, dv_l);

        let c_tr = quarter_ellipse_perimeter(r_tr) * cv_tr;
        let c_br = quarter_ellipse_perimeter(r_br) * cv_br;
        let c_bl = quarter_ellipse_perimeter(r_bl) * cv_bl;
        let c_tl = quarter_ellipse_perimeter(r_tl) * cv_tl;

        let upto_tr = s_t;
        let upto_r = upto_tr + c_tr;
        let upto_br = upto_r + s_r;
        let upto_b = upto_br + c_br;
        let upto_bl = upto_b + s_b;
        let upto_l = upto_bl + c_bl;
        let upto_tl = upto_l + s_l;
        max_t = upto_tl + c_tl;

        if is_near_rounded_corner {
            // The eccentric anomaly, scaled by the corner's own arc length, is what keeps the dash
            // rhythm continuous across an elliptical corner: the angle alone would run fast on the
            // short axis and slow on the long one.
            let radii = max(corner_radii, vec2<f32>(1e-6));
            let anomaly = atan2(corner_center_to_point.y / radii.y, corner_center_to_point.x / radii.x);
            let fraction = saturate(anomaly / (0.5 * M_PI));

            if center_to_point.x >= 0.0 {
                if center_to_point.y < 0.0 {
                    velocity = cv_tr;
                    t = upto_r - fraction * c_tr;
                } else {
                    velocity = cv_br;
                    t = upto_br + fraction * c_br;
                }
            } else {
                if center_to_point.y >= 0.0 {
                    velocity = cv_bl;
                    t = upto_l - fraction * c_bl;
                } else {
                    velocity = cv_tl;
                    t = upto_tl + fraction * c_tl;
                }
            }
        } else {
            let is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
            if is_horizontal {
                if center_to_point.y < 0.0 {
                    velocity = dv_t;
                    t = (local.x - r_tl.x) * velocity;
                } else {
                    velocity = dv_b;
                    t = upto_bl - (local.x - r_bl.x) * velocity;
                }
            } else {
                if center_to_point.x < 0.0 {
                    velocity = dv_l;
                    t = upto_tl - (local.y - r_tl.y) * velocity;
                } else {
                    velocity = dv_r;
                    t = upto_r + (local.y - r_tr.y) * velocity;
                }
            }
        }
    }

    let dash_length = dash_per_width / period_per_width;
    // A straight run starts and ends with a dash, which is what shortening its extent by one dash
    // before dividing achieves.
    max_t -= select(0.0, dash_length, unrounded);
    if max_t >= 1.0 {
        let dash_count = floor(max_t);
        return dash_alpha(t, max_t / dash_count, dash_length, velocity, threshold);
    }
    if unrounded {
        let dash_gap = max_t - dash_length;
        if dash_gap > 0.0 {
            return dash_alpha(t, dash_length + dash_gap, dash_length, velocity, threshold);
        }
    }
    return 1.0;
}
