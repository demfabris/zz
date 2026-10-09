// Liquid glass. One module for every renderer: wgpu loads it as is and Metal
// translates it through naga.
//
// A glass batch runs in three steps. The frame under each glass region is
// copied into level 0 of a chain of textures that spans just those regions,
// dual Kawase passes blur them down the chain and back up to level 1, and
// one draw per glass refracts, tints, and lights what the chain holds. The
// chain is addressed in device pixels from its origin, scaled down by its
// level, so regions keep their places at every level.

struct GlassShape {
    // origin.xy, size.zw in device pixels
    rect: vec4<f32>,
    // top_left, top_right, bottom_right, bottom_left
    radii: vec4<f32>,
}

struct Glass {
    shapes: array<GlassShape, 8>,
    // The rectangle drawn: origin.xy, size.zw.
    quad: vec4<f32>,
    // Level 0 texels holding this glass's backdrop: min.xy, max.zw.
    backdrop: vec4<f32>,
    // Straight color mixed over the backdrop, alpha as its strength.
    tint: vec4<f32>,
    // bezel width, refraction at the edge, dispersion, blurred level (0 = sharp)
    optics: vec4<f32>,
    // light direction xy, glint strength, glint width
    light: vec4<f32>,
    // saturation, brightness, contrast, grain
    tone: vec4<f32>,
    // fresnel glow, edge shadow strength, edge shadow width, opacity
    rim: vec4<f32>,
    // shape count, corner smoothing, merge radius, pad
    shape: vec4<f32>,
    // touch glow center xy, radius, strength
    glow: vec4<f32>,
    // viewport size, then where level 0 of the chain starts in the frame
    viewport: vec4<f32>,
    // the window's rounded clip: rect, radii, then smoothing and whether set
    mask_rect: vec4<f32>,
    mask_radii: vec4<f32>,
    mask: vec4<f32>,
}

struct Blur {
    // Source texels holding valid data: min.xy, max.zw.
    source: vec4<f32>,
    // tap offset in source texels, pad
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> glass: Glass;
@group(0) @binding(1) var glass_sampler: sampler;
@group(0) @binding(2) var sharp_backdrop: texture_2d<f32>;
@group(0) @binding(3) var blurred_backdrop: texture_2d<f32>;

@group(0) @binding(4) var<uniform> blur: Blur;
@group(0) @binding(5) var blur_sampler: sampler;
@group(0) @binding(6) var blur_source: texture_2d<f32>;

// -- blur ------------------------------------------------------------------

@vertex
fn vs_glass_blur(@builtin(vertex_index) vertex_id: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vertex_id << 1u) & 2u), f32(vertex_id & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

fn blur_tap(position: vec2<f32>) -> vec4<f32> {
    let clamped = clamp(position, blur.source.xy + 0.5, blur.source.zw - 0.5);
    let size = vec2<f32>(textureDimensions(blur_source));
    return textureSampleLevel(blur_source, blur_sampler, clamped / size, 0.0);
}

// Halves the resolution: the center twice over and four diagonal taps, each
// a bilinear average of four texels.
@fragment
fn fs_glass_down(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let center = position.xy * 2.0;
    let o = blur.params.x;
    var sum = blur_tap(center) * 4.0;
    sum += blur_tap(center + vec2<f32>(-o, -o));
    sum += blur_tap(center + vec2<f32>(o, o));
    sum += blur_tap(center + vec2<f32>(o, -o));
    sum += blur_tap(center + vec2<f32>(-o, o));
    return sum / 8.0;
}

// Doubles the resolution with a tent of eight taps around the source point.
@fragment
fn fs_glass_up(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let center = position.xy * 0.5;
    let o = blur.params.x;
    var sum = blur_tap(center + vec2<f32>(-2.0 * o, 0.0));
    sum += blur_tap(center + vec2<f32>(2.0 * o, 0.0));
    sum += blur_tap(center + vec2<f32>(0.0, -2.0 * o));
    sum += blur_tap(center + vec2<f32>(0.0, 2.0 * o));
    sum += blur_tap(center + vec2<f32>(-o, -o)) * 2.0;
    sum += blur_tap(center + vec2<f32>(o, -o)) * 2.0;
    sum += blur_tap(center + vec2<f32>(-o, o)) * 2.0;
    sum += blur_tap(center + vec2<f32>(o, o)) * 2.0;
    return sum / 12.0;
}

// -- glass -----------------------------------------------------------------

@vertex
fn vs_glass(@builtin(vertex_index) vertex_id: u32) -> @builtin(position) vec4<f32> {
    let unit = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let position = glass.quad.xy + unit * glass.quad.zw;
    let device = position / glass.viewport.xy * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(device, 0.0, 1.0);
}

// Signed distance to a rounded rectangle (negative inside) and its outward
// normal. Corners above smoothing 2 are superellipses, measured as a p-norm,
// except on pills and circles. The normal comes from the same rectangle with
// corners rounded at least as wide as the bezel, so it turns smoothly through
// a tight corner instead of creasing along its diagonal.
fn rounded_rect(point: vec2<f32>, shape: GlassShape, smoothing: f32, bezel: f32) -> vec3<f32> {
    let half_size = shape.rect.zw * 0.5;
    let center_to_point = point - (shape.rect.xy + half_size);
    var radius: f32;
    if (center_to_point.x < 0.0) {
        radius = select(shape.radii.w, shape.radii.x, center_to_point.y < 0.0);
    } else {
        radius = select(shape.radii.z, shape.radii.y, center_to_point.y < 0.0);
    }
    let half_minor = min(half_size.x, half_size.y);
    radius = min(radius, half_minor);
    let exponent = select(smoothing, 2.0, radius >= half_minor - 0.01);
    let side = select(vec2<f32>(1.0), vec2<f32>(-1.0), center_to_point < vec2<f32>(0.0));
    let corner = abs(center_to_point) - half_size;

    let q = corner + radius;
    let outset = max(q, vec2<f32>(0.0));
    var distance: f32;
    if (outset.x > 0.0 && outset.y > 0.0) {
        if (exponent <= 2.001) {
            distance = length(outset);
        } else {
            distance = pow(pow(outset.x, exponent) + pow(outset.y, exponent), 1.0 / exponent);
        }
    } else {
        distance = max(q.x, q.y);
    }

    let soft = corner + min(max(radius, bezel), half_minor);
    let soft_outset = max(soft, vec2<f32>(0.0));
    var normal: vec2<f32>;
    if (soft_outset.x > 0.0 && soft_outset.y > 0.0) {
        if (exponent <= 2.001) {
            normal = soft_outset;
        } else {
            normal = pow(soft_outset, vec2<f32>(exponent - 1.0));
        }
    } else if (soft.x > soft.y) {
        normal = vec2<f32>(1.0, 0.0);
    } else {
        normal = vec2<f32>(0.0, 1.0);
    }
    return vec3<f32>(distance - radius, normalize(normal) * side);
}

// Smooth union of every shape, so shapes closer than the merge radius melt
// into one body. The radius shrinks where the two edges face the same way,
// which keeps neighbors in a row from bulging along their shared side.
fn glass_field(point: vec2<f32>) -> vec3<f32> {
    let count = u32(glass.shape.x);
    let smoothing = glass.shape.y;
    let merge = glass.shape.z;
    let bezel = glass.optics.x;
    var field = rounded_rect(point, glass.shapes[0], smoothing, bezel);
    for (var i = 1u; i < count; i += 1u) {
        let next = rounded_rect(point, glass.shapes[i], smoothing, bezel);
        let k = max(merge * min(0.5 * length(next.yz - field.yz), 1.0), 1e-4);
        let h = clamp(0.5 + 0.5 * (next.x - field.x) / k, 0.0, 1.0);
        let distance = mix(next.x, field.x, h) - k * h * (1.0 - h);
        let gradient = mix(next.yz, field.yz, h);
        field = vec3<f32>(distance, gradient);
    }
    return field;
}

fn backdrop(frame_position: vec2<f32>) -> vec4<f32> {
    let position = frame_position - glass.viewport.zw;
    let level = glass.optics.w;
    if (level < 0.5) {
        let clamped = clamp(position, glass.backdrop.xy + 0.5, glass.backdrop.zw - 0.5);
        let size = vec2<f32>(textureDimensions(sharp_backdrop));
        return textureSampleLevel(sharp_backdrop, glass_sampler, clamped / size, 0.0);
    }
    let scale = exp2(-level);
    let low = floor(glass.backdrop.xy * scale) + 0.5;
    let high = ceil(glass.backdrop.zw * scale) - 0.5;
    let clamped = clamp(position * scale, low, high);
    let size = vec2<f32>(textureDimensions(blurred_backdrop));
    return textureSampleLevel(blurred_backdrop, glass_sampler, clamped / size, 0.0);
}

fn grain(position: vec2<f32>) -> f32 {
    var state = u32(position.x) * 747796405u + u32(position.y) * 2891336453u;
    state = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    state = (state >> 22u) ^ state;
    return f32(state) / 4294967295.0 - 0.5;
}

@fragment
fn fs_glass(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let position = frag.xy;
    let field = glass_field(position);
    let distance = field.x;
    let normal = field.yz / max(length(field.yz), 1e-5);
    let light = glass.light.xy;
    // Where the edge faces along the light axis, either way round.
    let facing = abs(dot(normal, light));

    // A thin dark contour just outside the edge, strongest along the light
    // axis, separates the glass from what it floats over.
    let contour_width = glass.rim.z;
    var contour = 0.0;
    if (contour_width > 0.0) {
        contour = glass.rim.y * (1.0 - saturate(distance / contour_width)) * mix(0.35, 1.0, facing);
    }
    var clip = 1.0;
    if (glass.mask.y > 0.5) {
        let window = GlassShape(glass.mask_rect, glass.mask_radii);
        clip = saturate(0.5 - rounded_rect(position, window, glass.mask.x, 0.0).x);
    }
    contour = contour * clip;
    let coverage = saturate(0.5 - distance) * clip;
    if (coverage <= 0.0) {
        if (contour <= 0.0) {
            discard;
        }
        let alpha = contour * glass.rim.w;
        return vec4<f32>(0.0, 0.0, 0.0, alpha);
    }

    // The rim lenses the backdrop. Across the bezel the pull grows like a
    // quarter circle, from nothing where the flat face starts to the full
    // refraction at the edge, so the rim shows a squeezed, mirrored copy of
    // what lies further in.
    let depth = max(-distance, 0.0);
    let bezel = max(glass.optics.x, 1e-3);
    let edge = 1.0 - saturate(depth / bezel);
    let pull = glass.optics.y * (1.0 - sqrt(max(1.0 - edge * edge, 0.0)));
    let offset = -normal * pull;

    var color: vec4<f32>;
    let dispersion = glass.optics.z * pull;
    if (dispersion > 0.25) {
        let red = backdrop(position + offset - normal * dispersion * 0.5);
        let green = backdrop(position + offset);
        let blue = backdrop(position + offset + normal * dispersion * 0.5);
        color = vec4<f32>(red.r, green.g, blue.b, green.a);
    } else {
        color = backdrop(position + offset);
    }

    // Every renderer blends into its target the same way, leaving it
    // premultiplied, whether or not the surface composites that way.
    var rgb = color.rgb / max(color.a, 1e-4);
    let luminance = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luminance), rgb, glass.tone.x);
    rgb = (rgb - 0.5) * glass.tone.z + 0.5 + glass.tone.y;
    rgb = mix(rgb, glass.tint.rgb, glass.tint.a);

    // The glint: a thin bright line along the edge where it faces the light
    // axis, with a faint bleed inward, and a glow that fills the bezel.
    if (edge > 0.0) {
        let glint_width = max(glass.light.w, 1e-3);
        let line = saturate(1.0 - depth / glint_width) + 0.2 * saturate(1.0 - depth / (glint_width * 4.0));
        let lobe = facing * facing;
        rgb = rgb + glass.light.z * line * mix(0.15, 1.0, lobe);
        rgb = rgb + glass.rim.x * edge * edge * edge;
    }

    // Touch lights the glass from inside, brightest under the finger.
    if (glass.glow.w > 0.0) {
        let reach = 1.0 - saturate(length(position - glass.glow.xy) / max(glass.glow.z, 1.0));
        rgb = rgb + glass.glow.w * reach * reach;
    }

    rgb = rgb + grain(position) * glass.tone.w;
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let body = (color.a + (1.0 - color.a) * glass.tint.a) * coverage * glass.rim.w;
    let alpha = body + (1.0 - body) * contour * glass.rim.w;
    return vec4<f32>(rgb * body, alpha);
}
