// Wave ordered dither — resident storage textures (Path B).
// Ports CPU dither_ordered::wave_threshold + Bayer-style quantize.

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.283185307179586;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct WaveUniforms {
    tile: TileUniforms,
    // x=levels, y=threshold_scale, z=color_mode, w=threshold_bias
    params: vec4<f32>,
}

struct WaveShapeUniforms {
    // x=wavelength, y=amplitude, z=phase, w=wave_angle_deg
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: WaveUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<uniform> wave: WaveShapeUniforms;

fn apply_threshold_bias(threshold: f32, bias: f32) -> f32 {
    if (bias == 0.0) {
        return threshold;
    }
    return clamp(threshold + bias, 0.0, 0.999999);
}

fn wave_threshold(gx: f32, gy: f32) -> f32 {
    let phi = wave.packed.w * PI / 180.0;
    let uu = gx * cos(phi) + gy * sin(phi);
    let wl = max(wave.packed.x, 1e-6);
    let t = 0.5 + 0.5 * sin(TAU * uu / wl + wave.packed.z) * wave.packed.y;
    return clamp(t, 0.0, 0.999999);
}

fn round_away_from_zero(x: f32) -> f32 {
    if (x < 0.0) {
        return ceil(x - 0.5);
    }
    return floor(x + 0.5);
}

fn quantize_uniform(value: f32, levels: f32, offset: f32) -> f32 {
    let scaled = value * (levels - 1.0) + offset;
    return clamp(round_away_from_zero(scaled), 0.0, levels - 1.0) / (levels - 1.0);
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn dither_at(gid: vec3<u32>, threshold: f32) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let levels = u.params.x;
    let threshold_scale = u.params.y;
    let color_mode = u.params.z;
    let dither_a = color_mode >= 1.5;
    let is_gray = (color_mode % 2.0) >= 0.5;
    let offset = (threshold - 0.5) * threshold_scale;

    var out = rgba;
    if (!is_gray) {
        out.r = quantize_uniform(rgba.r, levels, offset);
        out.g = quantize_uniform(rgba.g, levels, offset);
        out.b = quantize_uniform(rgba.b, levels, offset);
    } else {
        let lum = luminance(rgba.r, rgba.g, rgba.b);
        let q = quantize_uniform(lum, levels, offset);
        out = vec4<f32>(q, q, q, rgba.a);
    }
    if (dither_a) {
        if (rgba.a <= 0.0) {
            out.a = 0.0;
        } else if (rgba.a >= 1.0) {
            out.a = 1.0;
        } else if (rgba.a > threshold) {
            out.a = 1.0;
        } else {
            out.a = 0.0;
        }
    }
    if (dither_a && out.a <= 0.0) {
        out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = f32(u.tile.tile_offset.x + gid.x);
    let gy = f32(u.tile.tile_offset.y + gid.y);
    let t = apply_threshold_bias(wave_threshold(gx, gy), u.params.w);
    dither_at(gid, t);
}
