// DERIVED-FROM: the GPUI project, crates/gpui_wgpu/src/shaders.wgsl (Apache-2.0)
// The analytic blurred rounded rectangle — the `erf` approximation, the closed-form horizontal
// integral, the four-sample vertical accumulation and the inset complement — is adapted from that
// work, which is licensed under the Apache License, Version 2.0, and has been modified: the
// scanline's horizontal extent is the ellipse equation rather than the circle's, so a corner with
// two different semi-axes blurs correctly, and it reduces to that work's exact expression when the
// two are equal.

struct Shadow {
    order: u32,
    blur: f32,
    bounds: Bounds,
    radii: Radii,
    element_bounds: Bounds,
    element_radii: Radii,
    color: Rgba,
    clip: u32,
    transform: u32,
    inset: u32,
    // The superellipse exponent the element's corners are cut with; two is the ellipse.
    shape: f32,
}

@group(1) @binding(0) var shadows: texture_2d<u32>;

/// One shadow, which spans 9 texels of the arena.
fn load_shadow(slot: u32) -> Shadow {
    let base = slot * 9u;
    let t0 = textureLoad(shadows, table_texel(base + 0u), 0);
    let t1 = textureLoad(shadows, table_texel(base + 1u), 0);
    let t2 = textureLoad(shadows, table_texel(base + 2u), 0);
    let t3 = textureLoad(shadows, table_texel(base + 3u), 0);
    let t4 = textureLoad(shadows, table_texel(base + 4u), 0);
    let t5 = textureLoad(shadows, table_texel(base + 5u), 0);
    let t6 = textureLoad(shadows, table_texel(base + 6u), 0);
    let t7 = textureLoad(shadows, table_texel(base + 7u), 0);
    let t8 = textureLoad(shadows, table_texel(base + 8u), 0);
    return Shadow(
        t0.x,
        bitcast<f32>(t0.y),
        Bounds(bitcast<f32>(t0.z), bitcast<f32>(t0.w), bitcast<f32>(t1.x), bitcast<f32>(t1.y)),
        Radii(bitcast<f32>(t1.z), bitcast<f32>(t1.w), bitcast<f32>(t2.x), bitcast<f32>(t2.y), bitcast<f32>(t2.z), bitcast<f32>(t2.w), bitcast<f32>(t3.x), bitcast<f32>(t3.y)),
        Bounds(bitcast<f32>(t3.z), bitcast<f32>(t3.w), bitcast<f32>(t4.x), bitcast<f32>(t4.y)),
        Radii(bitcast<f32>(t4.z), bitcast<f32>(t4.w), bitcast<f32>(t5.x), bitcast<f32>(t5.y), bitcast<f32>(t5.z), bitcast<f32>(t5.w), bitcast<f32>(t6.x), bitcast<f32>(t6.y)),
        Rgba(bitcast<f32>(t6.z), bitcast<f32>(t6.w), bitcast<f32>(t7.x), bitcast<f32>(t7.y)),
        t7.z,
        t7.w,
        t8.x,
        bitcast<f32>(t8.y),
    );
}

// The record travels with the vertices rather than being fetched again per fragment.
//
// It is one value for the whole instance, so the four vertices already know all of it — and the
// fragment shader was reading all nine of its texels again for every pixel it covered. On a device
// whose tables are textures those nine fetches were 42% of the composition pass, three times what
// the whole blur integral costs. Flat interpolation is the same value delivered without them.
struct ShadowVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) bounds: vec4<f32>,
    @location(2) @interpolate(flat) radii_near: vec4<f32>,
    @location(3) @interpolate(flat) radii_far: vec4<f32>,
    @location(4) @interpolate(flat) element_bounds: vec4<f32>,
    @location(5) @interpolate(flat) element_radii_near: vec4<f32>,
    @location(6) @interpolate(flat) element_radii_far: vec4<f32>,
    @location(7) @interpolate(flat) color: vec4<f32>,
    // The clip's own box, and in `misc` the blur, whether the shadow is inset, and how many
    // rounded tests the clip carries. The clip is read here for the same reason the record is:
    // nearly every primitive is clipped by nothing but a box, and answering that per fragment was
    // two more texture reads for a result the whole instance shares.
    @location(8) @interpolate(flat) clip_box: vec4<f32>,
    @location(9) @interpolate(flat) misc: vec4<f32>,
    @location(10) @interpolate(flat) shift: vec2<f32>,
    @location(11) @interpolate(flat) shape: f32,
}

// The record put back together from what the vertices carried.
//
// `order`, `transform` and `reserved` are the vertex stage's alone — one decides nothing here, one
// has already been applied to the position, and one is padding — so they are not carried and are
// filled with zero.
fn shadow_of(in: ShadowVarying) -> Shadow {
    return Shadow(
        0u,
        in.misc.x,
        Bounds(in.bounds.x, in.bounds.y, in.bounds.z, in.bounds.w),
        Radii(
            in.radii_near.x, in.radii_near.y, in.radii_near.z, in.radii_near.w,
            in.radii_far.x, in.radii_far.y, in.radii_far.z, in.radii_far.w,
        ),
        Bounds(
            in.element_bounds.x, in.element_bounds.y, in.element_bounds.z, in.element_bounds.w,
        ),
        Radii(
            in.element_radii_near.x, in.element_radii_near.y,
            in.element_radii_near.z, in.element_radii_near.w,
            in.element_radii_far.x, in.element_radii_far.y,
            in.element_radii_far.z, in.element_radii_far.w,
        ),
        Rgba(in.color.x, in.color.y, in.color.z, in.color.w),
        0u,
        0u,
        u32(in.misc.y),
        in.shape,
    );
}

// Coverage by the clip, from what the vertices carried.
//
// A clip that is a box and nothing else — which is nearly all of them — is settled by the box test
// alone, and the rounded tests below it are never reached. The two texture reads that used to
// answer this per fragment are gone; only a clip that really rounds still reads a table.
fn clip_from(in: ShadowVarying, point: vec2<f32>) -> f32 {
    let box = in.clip_box;
    if point.x < box.x || point.y < box.y || point.x > box.x + box.z || point.y > box.y + box.w {
        return 0.0;
    }
    return clip_rounded_coverage(point, u32(in.misc.w), u32(in.misc.z));
}

@vertex
fn vs_shadow(
    @builtin(vertex_index) vertex: u32,
    @location(0) slot: u32,
    @location(1) shift: vec2<f32>,
) -> ShadowVarying {
    let shadow = load_shadow(slot);
    // `bounds` is already everything the primitive paints: the blurred shape dilated by the
    // gaussian's reach for a drop shadow, and the casting box itself for an inset one.
    let local = inflated_corner(vertex, shadow.bounds) + shift;
    var out: ShadowVarying;
    out.position = to_clip_position(local, shadow.transform);
    out.local = local;
    let b = shadow.bounds;
    out.bounds = vec4<f32>(b.x, b.y, b.w, b.h);
    let r = shadow.radii;
    out.radii_near = vec4<f32>(r.tl_x, r.tl_y, r.tr_x, r.tr_y);
    out.radii_far = vec4<f32>(r.br_x, r.br_y, r.bl_x, r.bl_y);
    let e = shadow.element_bounds;
    out.element_bounds = vec4<f32>(e.x, e.y, e.w, e.h);
    let q = shadow.element_radii;
    out.element_radii_near = vec4<f32>(q.tl_x, q.tl_y, q.tr_x, q.tr_y);
    out.element_radii_far = vec4<f32>(q.br_x, q.br_y, q.bl_x, q.bl_y);
    let c = shadow.color;
    out.color = vec4<f32>(c.r, c.g, c.b, c.a);
    out.clip_box = bitcast<vec4<f32>>(textureLoad(clips, table_texel(shadow.clip * 11u + 0u), 0));
    let rounds = textureLoad(clips, table_texel(shadow.clip * 11u + 9u), 0).x;
    out.misc = vec4<f32>(shadow.blur, f32(shadow.inset), f32(rounds), f32(shadow.clip));
    out.shift = shift;
    out.shape = shadow.shape;
    return out;
}

// A standard gaussian, used to weight the vertical samples.
fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * M_PI) * sigma);
}

// An approximation of the error function, which is the integral the gaussian needs.
fn erf(v: vec2<f32>) -> vec2<f32> {
    let s = sign(v);
    let a = abs(v);
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let r2 = r1 * r1;
    return s - s / (r2 * r2);
}

// The blurred coverage of one scanline of a rounded rectangle, exactly analytic in x.
//
// `curved` is where the shape's edge sits on this scanline. With an elliptical corner that is the
// ellipse equation rather than the circle's, and it reduces to the circular form when the two
// semi-axes are equal — so the generalisation costs one division and no samples. A corner cut to
// any other exponent takes the superellipse's own edge, which reduces to the ellipse's at two.
fn blur_along_x(
    x: f32,
    y: f32,
    sigma: f32,
    corner: vec2<f32>,
    half_size: vec2<f32>,
    shape: f32,
) -> f32 {
    let delta = min(half_size.y - corner.y - abs(y), 0.0);
    var curved = half_size.x - corner.x;
    if corner.y > 0.0 {
        if shape == CORNER_ROUND {
            let normalised = saturate(1.0 - (delta * delta) / (corner.y * corner.y));
            curved += corner.x * sqrt(normalised);
        } else {
            // `|x/rx|^n + |y/ry|^n = 1` solved for x, which is the ellipse's own solution at two.
            let power = clamp(shape, 0.01, 64.0);
            let along = saturate(abs(delta) / corner.y);
            let remaining = saturate(1.0 - pow(along, power));
            curved += corner.x * pow(remaining, 1.0 / power);
        }
    } else {
        curved += corner.x;
    }
    let integral = 0.5 + 0.5 * erf((x + vec2<f32>(-curved, curved)) * (sqrt(0.5) / sigma));
    return integral.y - integral.x;
}

@fragment
fn fs_shadow(in: ShadowVarying) -> @location(0) vec4<f32> {
    let shadow = shadow_of(in);
    let clip = clip_from(in, device_position(in.position.xy));
    if clip <= 0.0 {
        return vec4<f32>(0.0);
    }

    // The blurred shape is the element box, offset and spread; `bounds` is that shape dilated by
    // the blur's reach, so the shape itself has to be recovered from it.
    let casting = shadow_shape(shadow);
    let half_size = bounds_size(casting) * 0.5;
    let center = bounds_origin(casting) + half_size;
    let local = in.local - in.shift;
    let center_to_point = local - center;
    let corner = pick_corner_radii(center_to_point, shadow.radii);

    // An outer shadow paints nothing inside the box that casts it — the multiply at the end of
    // this function says so, having already paid for the integral. Settled here instead, before
    // four samples of two error functions and an exponential apiece.
    let element_distance = quad_sdf(local, shadow.element_bounds, shadow.element_radii, shadow.shape);
    if shadow.inset == 0u && element_distance <= -0.5 {
        return vec4<f32>(0.0);
    }

    var alpha: f32;
    if shadow.blur <= 0.0 {
        alpha = saturate(0.5 - quad_sdf(local, casting, shadow.radii, shadow.shape));
    } else {
        // The gaussian is negligible beyond three standard deviations, and the shape contributes
        // nothing outside its own extent, so the samples are spent only where the two overlap.
        let low = center_to_point.y - half_size.y;
        let high = center_to_point.y + half_size.y;
        let start = clamp(-3.0 * shadow.blur, low, high);
        let end = clamp(3.0 * shadow.blur, low, high);
        let step = (end - start) / 4.0;
        var y = start + step * 0.5;
        alpha = 0.0;
        for (var i = 0; i < 4; i += 1) {
            let blurred = blur_along_x(
                center_to_point.x,
                center_to_point.y - y,
                shadow.blur,
                corner,
                half_size,
                shadow.shape,
            );
            alpha += blurred * gaussian(y, shadow.blur) * step;
            y += step;
        }
    }

    if shadow.inset != 0u {
        // An inset shadow is the complement of the blurred hole, clipped to the element it sits in.
        alpha = 1.0 - alpha;
        alpha *= saturate(0.5 - element_distance);
    } else {
        // An outer shadow is never painted within the box that casts it. Behind a filled box the
        // difference cannot be seen, but a box with no fill of its own — a field that is a hole in
        // the page — would otherwise wear its own shadow as a wash over its whole interior.
        alpha *= saturate(0.5 + element_distance);
    }

    return rgba_of(shadow.color) * alpha * clip;
}

// The rectangle the blur is applied to.
//
// A drop shadow's `bounds` is the shape dilated by the blur's reach on every side, so the shape is
// recovered by removing it. An inset shadow paints only inside the box that casts it, so its
// `bounds` is that box and is already the shape the blur is applied to.
fn shadow_shape(shadow: Shadow) -> Bounds {
    if shadow.inset != 0u {
        return shadow.bounds;
    }
    let reach = 3.0 * shadow.blur;
    return Bounds(
        shadow.bounds.x + reach,
        shadow.bounds.y + reach,
        max(shadow.bounds.w - 2.0 * reach, 0.0),
        max(shadow.bounds.h - 2.0 * reach, 0.0),
    );
}
