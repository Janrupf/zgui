// Compositing a rasterised vector batch back into the target, at exactly its point in the order.
//
// A path rasteriser cannot draw into the target directly: it writes a scratch texture of its own,
// and this is the ordinary draw that puts the result down. Because z-order here *is* submission
// order — there is no depth buffer, no stencil and no order-independent scheme anywhere — putting
// this draw at the right index in the batch stream is precisely and exactly right.
//
// One texel of the scratch is one device pixel, so the read is a `textureLoad` at an integer offset
// with no sampler at all, which keeps the bind-group layout clear of the filtering restrictions.
//
// The scratch holds **straight** — un-premultiplied — colour, because that is what the rasteriser
// contract says it holds. This is the one place in the renderer that converts it, and the result
// then blends premultiplied like every other draw.

struct VectorInstance {
    // The quad, in device pixels: origin then extent.
    bounds: vec4<f32>,
    // xy: the scratch texel the quad's origin reads. zw: unused.
    source: vec4<f32>,
    // x: the clip chain this instance binds. yzw: unused.
    control: vec4<f32>,
}

@group(1) @binding(0) var vector_instances: texture_2d<u32>;

/// One vector-composite instance, which spans 3 texels of the arena.
fn load_vector_instance(slot: u32) -> VectorInstance {
    let base = slot * 3u;
    let t0 = textureLoad(vector_instances, table_texel(base + 0u), 0);
    let t1 = textureLoad(vector_instances, table_texel(base + 1u), 0);
    let t2 = textureLoad(vector_instances, table_texel(base + 2u), 0);
    return VectorInstance(
        vec4<f32>(bitcast<f32>(t0.x), bitcast<f32>(t0.y), bitcast<f32>(t0.z), bitcast<f32>(t0.w)),
        vec4<f32>(bitcast<f32>(t1.x), bitcast<f32>(t1.y), bitcast<f32>(t1.z), bitcast<f32>(t1.w)),
        vec4<f32>(bitcast<f32>(t2.x), bitcast<f32>(t2.y), bitcast<f32>(t2.z), bitcast<f32>(t2.w)),
    );
}

@group(1) @binding(1) var vector_scratch: texture_2d<f32>;

struct VectorVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) instance: u32,
}

@vertex
fn vs_vector(
    @builtin(vertex_index) vertex: u32,
    @builtin(instance_index) instance: u32,
) -> VectorVarying {
    let record = load_vector_instance(instance);
    let device = record.bounds.xy + unit_corner(vertex) * record.bounds.zw;
    var out: VectorVarying;
    out.position = to_clip_position(device, 0u);
    out.instance = instance;
    return out;
}

@fragment
fn fs_vector(in: VectorVarying) -> @location(0) vec4<f32> {
    let record = load_vector_instance(in.instance);
    let device = device_position(in.position.xy);
    // The same chain evaluation every other pipeline applies, so vector content is clipped by
    // exactly what a quad would have been clipped by.
    let coverage = clip_coverage(device, u32(record.control.x));
    if coverage <= 0.0 {
        return vec4<f32>(0.0);
    }
    let texel = vec2<i32>(floor(device - record.bounds.xy + record.source.xy));
    let straight = textureLoad(vector_scratch, texel, 0);
    let premultiplied = vec4<f32>(straight.rgb * straight.a, straight.a);
    return premultiplied * coverage;
}
