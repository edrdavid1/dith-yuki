// Crosshatch engraving ladder — resident storage textures (Path B).
// Ports CPU filters::crosshatch (ps=1, no palette).

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const PI: f32 = 3.141592653589793;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct CrosshatchUniforms {
    tile: TileUniforms,
    // x=spacing, y=threshold_scale (thickness), z=dither_alpha, w=pattern_angle_deg
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: CrosshatchUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if (r < 0.0) {
        return r + b;
    }
    return r;
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn hatch_line_distance(gx: f32, gy: f32, angle_deg: f32, spacing: f32) -> f32 {
    let sp = max(spacing, 1.0);
    let phi = angle_deg * PI / 180.0;
    let uu = gx * cos(phi) + gy * sin(phi);
    let d = rem_euclid_f(uu, sp);
    return min(d, sp - d);
}

fn on_hatch_line(gx: f32, gy: f32, angle_deg: f32, spacing: f32, half_width: f32) -> bool {
    return hatch_line_distance(gx, gy, angle_deg, spacing) <= max(half_width, 0.25);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);

    let gx = f32(u.tile.tile_offset.x + gid.x);
    let gy = f32(u.tile.tile_offset.y + gid.y);
    let base_spacing = max(u.params.x, 2.0);
    let thickness_scale = max(u.params.y, 0.05);
    let dither_a = u.params.z > 0.5;
    let base_angle = u.params.w;
    let darkness = clamp(1.0 - luminance(rgba.r, rgba.g, rgba.b), 0.0, 1.0);

    // HATCH_LADDER: (thresh, ang_off, sp_mul)
    let thresh = array<f32, 4>(0.12, 0.38, 0.58, 0.82);
    let ang_off = array<f32, 4>(0.0, 0.0, 90.0, 45.0);
    let sp_mul = array<f32, 4>(1.0, 0.62, 0.62, 0.45);

    var ink = false;
    for (var i = 0u; i < 4u; i++) {
        if (darkness < thresh[i]) {
            continue;
        }
        let spacing = max(base_spacing * sp_mul[i], 2.0);
        let half_w = clamp(spacing * 0.15 * thickness_scale, 0.35, spacing * 0.45);
        if (on_hatch_line(gx, gy, base_angle + ang_off[i], spacing, half_w)) {
            ink = true;
            break;
        }
    }

    let v = select(1.0, 0.0, ink);
    var out = vec4<f32>(v, v, v, rgba.a);
    if (dither_a && rgba.a <= 0.0) {
        out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}
