// The vocabulary every pipeline shares: the frame's globals, the side tables addressed by index
// from every instance, and the plain-old-data spellings of the display list's structures.
//
// Every field of every instance structure is a four-byte scalar, and that is deliberate rather
// than clumsy: a `vec4<f32>` would carry a sixteen-byte alignment that the packed Rust structures
// do not have, so the two layouts would agree on some fields and silently disagree on others. The
// shader-reflection check compares these declarations against the Rust ones field by field.

struct Globals {
    // xy: the target's extent in texels.
    // zw: how many texels one device pixel covers — one in the composed target, a half in a
    //     half-resolution isolated one. Every primitive is positioned and clipped in device
    //     pixels whichever target it lands in, and this is the whole of the difference.
    viewport: vec4<f32>,
    // The four gamma-correction coefficients coverage is corrected with.
    gamma_ratios: vec4<f32>,
    // x: contrast enhancement for single-channel coverage.
    // y: contrast enhancement for per-channel coverage.
    // z: non-zero when the display's subpixels run blue to red.
    text: vec4<f32>,
    // x: seconds since the document started.
    // y: seconds the previous frame took.
    // z: device pixels per CSS pixel.
    // w: unused.
    //
    // Read by application effects and by nothing the framework draws, which is why it is one lane
    // of the block every pipeline already binds rather than a block of its own.
    frame: vec4<f32>,
}

// A box on the device pixel grid: origin then extent.
struct Bounds {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

// Per-corner elliptical radii, clockwise from the top left.
struct Radii {
    tl_x: f32,
    tl_y: f32,
    tr_x: f32,
    tr_y: f32,
    br_x: f32,
    br_y: f32,
    bl_x: f32,
    bl_y: f32,
}

// Premultiplied, gamma-encoded sRGB.
struct Rgba {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

// Widths of the four sides.
struct Edges {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

// A primitive's reference to its paint: a family and an index.
struct PaintRef {
    kind: u32,
    index: u32,
}

// Four floats whose meaning is whatever reads them.
struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

// Three floats, used to keep a structure free of padding.
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}

// Two floats: a point or a displacement, spelled as scalars so it carries no wider alignment than
// the packed structures it is a member of.
struct Vector2 {
    x: f32,
    y: f32,
}

// A rectangle of an atlas texture, in texels.
struct TileRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

// Where a cached raster lives.
struct Tile {
    texture: u32,
    tile: u32,
    bounds: TileRect,
}

// One rounded-rectangle test of a clip chain.
struct Rounded {
    rect: Bounds,
    radii: Radii,
    // The superellipse exponent the corners are cut with; two is the ellipse.
    shape: f32,
    // Padding to a whole texel, so the rectangle and the radii sit texel-aligned in the table and
    // `clip_coverage` can fetch each by texel. See `bind::tables::GpuRounded`.
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

// A whole clip chain, flattened into what one draw call applies.
//
// Nothing loads a whole one: `clip_coverage` in sdf.wgsl reads the record a texel at a time, and
// most fragments stop after two. The declaration is what the record's layout is checked against.
struct Clip {
    aabb: Bounds,
    first: Rounded,
    second: Rounded,
    count: u32,
    has_mask: u32,
    mask: Tile,
}

// One paint source.
struct Paint {
    // 0 nothing, 1 solid, 2 gradient, 3 image.
    kind: u32,
    // 0 linear, 1 radial, 2 conic.
    gradient: u32,
    // Which space the ramp's stops were written in: 0 encoded sRGB, 1 Oklab, 2 linear sRGB.
    space: u32,
    // 1 when the ramp repeats outside its extent.
    flags: u32,
    // Linear: start then end. Radial: centre then the two radii. Conic: centre, then start angle.
    geometry: Vector4,
    // The colour of a solid paint.
    color: Rgba,
    stop_start: u32,
    stop_count: u32,
    pad0: u32,
    pad1: u32,
}

// One stop of a ramp, in the space the ramp is interpolated in, alpha-premultiplied.
struct Stop {
    color: Vector4,
    offset: f32,
    pad: Vector3,
}

// One coordinate system: the matrix mapping it onto the device.
struct Spatial {
    matrix: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

// The side tables, as textures rather than storage buffers. Storage buffers arrive in OpenGL 4.3
// and OpenGL ES 3.1, so a GL 3.3 context has none and neither has WebGL 2; a texture read one texel
// at a time gives the same random access on every device. `buffer::tables` states what that costs.
//
// Every field of every table is four bytes and they are laid out in declaration order, so one
// `rgba32uint` texel is four consecutive fields and the loaders below are the structures spelled
// out. Each states how many texels its structure spans, and that number is the stride.
@group(0) @binding(1) var clips: texture_2d<u32>;
@group(0) @binding(2) var paints: texture_2d<u32>;
@group(0) @binding(3) var stops: texture_2d<u32>;
@group(0) @binding(4) var spatial: texture_2d<u32>;

/// How many texels wide every table is. `buffer::tables::TEXELS_WIDE` is the same number.
const TABLE_TEXELS_WIDE: u32 = 256u;

/// Where texel `index` of a table is.
fn table_texel(index: u32) -> vec2<i32> {
    return vec2<i32>(
        i32(index % TABLE_TEXELS_WIDE),
        i32(index / TABLE_TEXELS_WIDE),
    );
}

/// One paint, which spans four texels.
fn load_paint(id: u32) -> Paint {
    let base = id * 4u;
    let t0 = textureLoad(paints, table_texel(base + 0u), 0);
    let t1 = textureLoad(paints, table_texel(base + 1u), 0);
    let t2 = textureLoad(paints, table_texel(base + 2u), 0);
    let t3 = textureLoad(paints, table_texel(base + 3u), 0);
    return Paint(
        t0.x, t0.y, t0.z, t0.w,
        Vector4(bitcast<f32>(t1.x), bitcast<f32>(t1.y), bitcast<f32>(t1.z), bitcast<f32>(t1.w)),
        Rgba(bitcast<f32>(t2.x), bitcast<f32>(t2.y), bitcast<f32>(t2.z), bitcast<f32>(t2.w)),
        t3.x, t3.y, t3.z, t3.w,
    );
}

/// One ramp stop, which spans two texels.
fn load_stop(id: u32) -> Stop {
    let base = id * 2u;
    let t0 = textureLoad(stops, table_texel(base + 0u), 0);
    let t1 = textureLoad(stops, table_texel(base + 1u), 0);
    return Stop(
        Vector4(bitcast<f32>(t0.x), bitcast<f32>(t0.y), bitcast<f32>(t0.z), bitcast<f32>(t0.w)),
        bitcast<f32>(t1.x),
        Vector3(bitcast<f32>(t1.y), bitcast<f32>(t1.z), bitcast<f32>(t1.w)),
    );
}

/// One transform, which spans four texels: a `mat4x4<f32>` is four columns of four.
fn load_spatial(id: u32) -> mat4x4<f32> {
    let base = id * 4u;
    let t0 = textureLoad(spatial, table_texel(base + 0u), 0);
    let t1 = textureLoad(spatial, table_texel(base + 1u), 0);
    let t2 = textureLoad(spatial, table_texel(base + 2u), 0);
    let t3 = textureLoad(spatial, table_texel(base + 3u), 0);
    return mat4x4<f32>(
        bitcast<vec4<f32>>(t0),
        bitcast<vec4<f32>>(t1),
        bitcast<vec4<f32>>(t2),
        bitcast<vec4<f32>>(t3),
    );
}

const PAINT_NONE: u32 = 0u;
const PAINT_SOLID: u32 = 1u;
const PAINT_GRADIENT: u32 = 2u;
const PAINT_IMAGE: u32 = 3u;

const SPACE_SRGB: u32 = 0u;
const SPACE_OKLAB: u32 = 1u;
const SPACE_LINEAR_SRGB: u32 = 2u;

const M_PI: f32 = 3.141592653589793;

fn bounds_origin(b: Bounds) -> vec2<f32> {
    return vec2<f32>(b.x, b.y);
}

fn bounds_size(b: Bounds) -> vec2<f32> {
    return vec2<f32>(b.w, b.h);
}

fn rgba_of(c: Rgba) -> vec4<f32> {
    return vec4<f32>(c.r, c.g, c.b, c.a);
}

fn tile_bounds(t: Tile) -> vec4<f32> {
    return vec4<f32>(f32(t.bounds.x), f32(t.bounds.y), f32(t.bounds.w), f32(t.bounds.h));
}

fn vector4_of(v: Vector4) -> vec4<f32> {
    return vec4<f32>(v.x, v.y, v.z, v.w);
}

// The four corners of the unit square, in triangle-strip order.
fn unit_corner(vertex: u32) -> vec2<f32> {
    return vec2<f32>(f32(vertex & 1u), 0.5 * f32(vertex & 2u));
}

// A device-space point, transformed and projected into the current target's clip space.
fn to_clip_position(point: vec2<f32>, spatial_id: u32) -> vec4<f32> {
    let world = load_spatial(spatial_id) * vec4<f32>(point, 0.0, 1.0);
    let texel = world.xy * globals.viewport.zw;
    let ndc = vec2<f32>(
        texel.x / globals.viewport.x * 2.0 - world.w,
        world.w - texel.y / globals.viewport.y * 2.0,
    );
    return vec4<f32>(ndc, 0.0, world.w);
}

// The device pixel a fragment's own target coordinate names.
//
// Shapes and clips are in device pixels in every target, so a half-resolution target evaluates
// exactly the same geometry as the composed one — at half the sample rate, which is the entire
// difference between the two and the only one there should be.
fn device_position(position: vec2<f32>) -> vec2<f32> {
    return position / globals.viewport.zw;
}

// The device-space point a unit-square corner maps to, with one pixel of slack on every side so an
// antialiased edge has somewhere to land.
fn inflated_corner(vertex: u32, b: Bounds) -> vec2<f32> {
    let origin = bounds_origin(b) - vec2<f32>(1.0);
    let size = bounds_size(b) + vec2<f32>(2.0);
    return origin + unit_corner(vertex) * size;
}

// The chunk offsets that let a chunk which merely moved keep its resident bytes arrive resolved,
// as the second attribute of the order stream: the geometry shifts in the vertex stage, and a
// fragment stage comparing against encode-space fields subtracts the same shift from its sample
// point. The packing that once carried them lives in buffer/persist.rs, on the host alone.
