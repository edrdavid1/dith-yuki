// Line-screen halftone — resident storage textures (Path B).
// Ports CPU filters::line_screen (ps=1, no palette).

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const PI: f32 = 3.141592653589793;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct LineScreenUniforms {
    tile: TileUniforms,
    // x=cell_size, y=threshold_scale, z=dither_alpha, w=pattern_angle_deg
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: LineScreenUniforms;
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

fn line_screen_distance(gx: f32, gy: f32, cell_size: f32, angle_deg: f32) -> f32 {
    let s = max(cell_size, 1.0);
    let phi = angle_deg * PI / 180.0;
    let uu = gx * cos(phi) + gy * sin(phi);
    let d = rem_euclid_f(uu, s);
    return abs(d - s * 0.5);
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
    let cell = max(u.params.x, 1.0);
    let scale = max(u.params.y, 0.0);
    let dither_a = u.params.z > 0.5;
    let angle = u.params.w;

    let tone = clamp(1.0 - luminance(rgba.r, rgba.g, rgba.b), 0.0, 1.0);
    var ink = false;
    if (tone > 0.0) {
        let half = (cell * 0.5) * tone * scale;
        ink = line_screen_distance(gx, gy, cell, angle) <= half;
    }
    let v = select(1.0, 0.0, ink);
    var out = vec4<f32>(v, v, v, rgba.a);
    if (dither_a && rgba.a <= 0.0) {
        out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}
