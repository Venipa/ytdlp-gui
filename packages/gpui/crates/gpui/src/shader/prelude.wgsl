// The prelude GPUI puts in front of every custom shader. It draws each instance as a rounded
// rectangle, hands the user's `shade` function a `ShaderInput`, and multiplies what `shade`
// returns by the shape's antialiased coverage and the element's opacity.
//
// Everything here that does not start with `gpui_` is part of the shader API: the user source
// may call it but may not declare anything with the same name.

struct GpuiShaderGlobals {
    viewport_size: vec2<f32>,
    premultiplied_alpha: u32,
    pad: u32,
}

// Mirrors `ShaderQuad` in gpui's scene. Positions and radii are in device pixels.
struct GpuiShaderInstance {
    order: u32,
    shader: u32,
    opacity: f32,
    time: f32,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    mask_origin: vec2<f32>,
    mask_size: vec2<f32>,
    corner_radii: vec4<f32>,
    scale_factor: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    params: array<vec4<f32>, 4>,
    colors: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var<uniform> gpui_globals: GpuiShaderGlobals;

/// What `shade` is told about the pixel it colours.
struct ShaderInput {
    /// The pixel centre relative to the top left of the bounds, in logical pixels.
    position: vec2<f32>,
    /// The size of the bounds in logical pixels.
    size: vec2<f32>,
    /// `position / size`, from 0 at the top left to 1 at the bottom right.
    uv: vec2<f32>,
    /// The pixel centre relative to the top left of the bounds, in device pixels.
    device_position: vec2<f32>,
    /// The size of the bounds in device pixels.
    device_size: vec2<f32>,
    /// The pixel centre in the render target, in device pixels. Patterns that should stay fixed
    /// to the screen grid, such as dithering, key off this.
    frag_coord: vec2<f32>,
    /// Device pixels per logical pixel.
    scale_factor: f32,
    /// The time the app painted this instance with, in seconds.
    time: f32,
    /// Signed distance from the pixel to the rounded edge in logical pixels, negative inside.
    distance: f32,
    /// The corner radii in logical pixels: top left, top right, bottom right, bottom left.
    corner_radii: vec4<f32>,
    /// The sixteen parameters the app painted this instance with, four to a vector.
    params: array<vec4<f32>, 4>,
    /// The four colours the app painted this instance with, as sRGB with straight alpha.
    colors: array<vec4<f32>, 4>,
}

struct GpuiShaderVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) instance: u32,
}

/// Signed distance from `p`, measured from the centre of a rectangle, to the edge of that
/// rectangle with `half_size` and per corner `radii` (top left, top right, bottom right, bottom
/// left). Negative inside.
fn sd_rounded_rect(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    var radius = select(radii.z, radii.y, p.y < 0.0);
    if p.x < 0.0 {
        radius = select(radii.w, radii.x, p.y < 0.0);
    }
    let corner = abs(p) - half_size + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

/// How much of a pixel lies inside an edge `distance` logical pixels away, negative inside.
fn coverage(distance: f32, scale_factor: f32) -> f32 {
    return saturate(0.5 - distance * scale_factor);
}

/// Rotates `v` counterclockwise by `angle` radians in a y-down frame.
fn rotate(v: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * v.x + s * v.y, c * v.y - s * v.x);
}

/// A well mixed 32 bit hash of `x`.
fn hash_u32(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

/// A well mixed 32 bit hash of a lattice cell.
fn hash2d_u32(cell: vec2<i32>) -> u32 {
    return hash_u32(bitcast<u32>(cell.x) + bitcast<u32>(cell.y) * 0x9e3779b9u);
}

/// A uniform value in [0, 1) for a lattice cell.
fn hash2d(cell: vec2<i32>) -> f32 {
    let h = hash2d_u32(cell);
    return f32(h >> 8u) / 16777216.0;
}

/// Two uniform values in [0, 1) for a lattice cell.
fn hash2d_vec(cell: vec2<i32>) -> vec2<f32> {
    let h = hash2d_u32(cell);
    let g = hash_u32(h + 0x632be5abu);
    return vec2<f32>(f32(h >> 8u), f32(g >> 8u)) / 16777216.0;
}

// A value in [0, 1) for a lattice cell from float arithmetic alone, which GPUs run several
// times faster than the 32 bit integer multiplies of `hash_u32`. Two rounds of scaling by
// irrational constants and keeping the fraction scatter neighbouring cells well enough for
// smooth noise.
fn gpui_cell_hash(cell: vec2<i32>) -> f32 {
    var p = fract(vec2<f32>(cell) * vec2<f32>(0.1618034, 0.2718282));
    p += dot(p, p.yx + vec2<f32>(41.31, 17.47));
    return fract(p.x * p.y * 95.4307);
}

fn gpui_quintic(t: vec2<f32>) -> vec2<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

/// Smooth value noise in [0, 1] with features about one unit apart.
fn value_noise(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    let f = gpui_quintic(fract(p));
    let a = gpui_cell_hash(cell);
    let b = gpui_cell_hash(cell + vec2<i32>(1, 0));
    let c = gpui_cell_hash(cell + vec2<i32>(0, 1));
    let d = gpui_cell_hash(cell + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// One of eight unit gradients for a lattice cell, picked by its hash, which avoids the cost of
// turning a random angle into a direction.
fn gpui_gradient(cell: vec2<i32>, offset: vec2<f32>) -> f32 {
    let h = u32(gpui_cell_hash(cell) * 16.0);
    let x = select(-1.0, 1.0, (h & 1u) != 0u);
    let y = select(-1.0, 1.0, (h & 2u) != 0u);
    let axis = select(vec2<f32>(x, 0.0), vec2<f32>(0.0, y), (h & 8u) != 0u);
    let gradient = select(axis, vec2<f32>(x, y) * 0.70710678, (h & 4u) != 0u);
    return dot(gradient, offset);
}

/// Smooth gradient noise, roughly in [-1, 1], with features about one unit apart.
fn gradient_noise(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    let local = fract(p);
    let f = gpui_quintic(local);
    let a = gpui_gradient(cell, local);
    let b = gpui_gradient(cell + vec2<i32>(1, 0), local - vec2<f32>(1.0, 0.0));
    let c = gpui_gradient(cell + vec2<i32>(0, 1), local - vec2<f32>(0.0, 1.0));
    let d = gpui_gradient(cell + vec2<i32>(1, 1), local - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y) * 1.4142;
}

/// Gradient noise summed over `octaves` octaves, each twice as fine and half as strong as the
/// one before and turned so their grids do not line up. Roughly in [-1, 1].
fn fbm(p: vec2<f32>, octaves: i32) -> f32 {
    var total = 0.0;
    var amplitude = 0.5;
    var point = p;
    var weight = 0.0;
    for (var octave = 0; octave < octaves; octave++) {
        total += gradient_noise(point) * amplitude;
        weight += amplitude;
        point = rotate(point, 0.6435) * 2.03 + vec2<f32>(17.13, -9.71);
        amplitude *= 0.5;
    }
    return total / max(weight, 1e-4);
}

/// The ordered dither threshold of `cell` in a Bayer matrix of side `2^bits`, in (0, 1).
/// `bits` is clamped to 1 through 4, a 2x2 to a 16x16 matrix.
fn bayer(cell: vec2<u32>, bits: u32) -> f32 {
    let depth = clamp(bits, 1u, 4u);
    var rank = 0u;
    for (var bit = 0u; bit < depth; bit++) {
        let x = (cell.x >> bit) & 1u;
        let y = (cell.y >> bit) & 1u;
        rank = rank * 4u + 2u * (x ^ y) + y;
    }
    let side = f32(1u << depth);
    return (f32(rank) + 0.5) / (side * side);
}

/// Noise in [0, 1) that is evenly spread across neighbouring pixels, for dithering.
fn interleaved_noise(frag_coord: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(frag_coord), vec2<f32>(0.06711056, 0.00583715))));
}

/// Adds a triangular dither of one 8 bit step to an sRGB colour so smooth gradients do not band.
fn dither(rgb: vec3<f32>, frag_coord: vec2<f32>) -> vec3<f32> {
    let a = interleaved_noise(frag_coord);
    let b = interleaved_noise(frag_coord + vec2<f32>(113.0, 71.0));
    return rgb + vec3<f32>((a + b - 1.0) / 255.0);
}

/// Converts sRGB encoded components to linear light.
fn srgb_to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

/// Converts linear light to sRGB encoded components.
fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let clamped = max(linear, vec3<f32>(0.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

/// Converts linear sRGB to Oklab.
fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let lms = pow(max(vec3<f32>(l, m, s), vec3<f32>(0.0)), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
        1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
        0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z,
    );
}

/// Converts Oklab to linear sRGB.
fn oklab_to_linear(c: vec3<f32>) -> vec3<f32> {
    let l = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let lms = vec3<f32>(l, m, s);
    let cubed = lms * lms * lms;
    return vec3<f32>(
        4.0767416621 * cubed.x - 3.3077115913 * cubed.y + 0.2309699292 * cubed.z,
        -1.2684380046 * cubed.x + 2.6097574011 * cubed.y - 0.3413193965 * cubed.z,
        -0.0041960863 * cubed.x - 0.7034186147 * cubed.y + 1.7076147010 * cubed.z,
    );
}

/// Mixes two sRGB colours through Oklab, which keeps the midpoint as bright and saturated as
/// the eye expects.
fn mix_oklab(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    let start = linear_to_oklab(srgb_to_linear(a));
    let end = linear_to_oklab(srgb_to_linear(b));
    return linear_to_srgb(oklab_to_linear(mix(start, end, t)));
}

@vertex
fn gpui_shader_vertex(
    @builtin(vertex_index) vertex_id: u32,
    @builtin(instance_index) instance_id: u32,
) -> GpuiShaderVarying {
    let instance = gpui_load_shader_instance(instance_id);
    // The quad is cut down to the content mask, so nothing outside it is shaded.
    let low = max(instance.bounds_origin, instance.mask_origin);
    let high = max(
        min(instance.bounds_origin + instance.bounds_size, instance.mask_origin + instance.mask_size),
        low,
    );
    let unit = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let position = mix(low, high, unit);
    let device = position / gpui_globals.viewport_size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);

    var out: GpuiShaderVarying;
    out.position = vec4<f32>(device, 0.0, 1.0);
    out.instance = instance_id;
    return out;
}

@fragment
fn gpui_shader_fragment(fragment: GpuiShaderVarying) -> @location(0) vec4<f32> {
    let instance = gpui_load_shader_instance(fragment.instance);
    let scale = max(instance.scale_factor, 1e-4);
    let device_position = fragment.position.xy - instance.bounds_origin;
    let half_size = instance.bounds_size * 0.5;
    let device_distance = sd_rounded_rect(device_position - half_size, half_size, instance.corner_radii);

    var input: ShaderInput;
    input.position = device_position / scale;
    input.size = instance.bounds_size / scale;
    input.uv = device_position / max(instance.bounds_size, vec2<f32>(1e-4));
    input.device_position = device_position;
    input.device_size = instance.bounds_size;
    input.frag_coord = fragment.position.xy;
    input.scale_factor = scale;
    input.time = instance.time;
    input.distance = device_distance / scale;
    input.corner_radii = instance.corner_radii / scale;
    input.params = instance.params;
    input.colors = instance.colors;

    let color = shade(input);
    let alpha = saturate(color.a) * saturate(0.5 - device_distance) * instance.opacity;
    let multiplier = select(1.0, alpha, gpui_globals.premultiplied_alpha != 0u);
    return vec4<f32>(saturate(color.rgb) * multiplier, alpha);
}
