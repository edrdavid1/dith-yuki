// Void-and-cluster on GPU-resident storage textures (Path B).
// Same quantize / rotate / bias path as Bayer resident; thresholds from ranks[].

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const VAC_SIZE: i32 = 64;
const VAC_N: f32 = 4096.0;
const ROT_FLOOR_BIAS: f32 = 5e-6;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct VacUniforms {
    tile: TileUniforms,
    // x=levels, y=threshold_scale
    params: vec4<f32>,
}

struct VacPatternUniforms {
    // x=pattern_sin, y=pattern_cos, z=color_mode, w=threshold_bias
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: VacUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> pat: VacPatternUniforms;
@group(0) @binding(4) var<storage, read> ranks: array<u32>;

fn rem_euclid_i32(a: i32, n: i32) -> i32 {
    let r = a % n;
    if (r < 0) {
        return r + n;
    }
    return r;
}

fn rotate_with_trig(gx: i32, gy: i32, sin_t: f32, cos_t: f32) -> vec2<i32> {
    if (abs(sin_t) < 0.0001) {
        return vec2<i32>(gx, gy);
    }
    let x = f32(gx);
    let y = f32(gy);
    let xr = x * cos_t - y * sin_t;
    let yr = x * sin_t + y * cos_t;
    return vec2<i32>(
        i32(floor(xr - ROT_FLOOR_BIAS)),
        i32(floor(yr - ROT_FLOOR_BIAS)),
    );
}

fn apply_threshold_bias(threshold: f32, bias: f32) -> f32 {
    if (bias == 0.0) {
        return threshold;
    }
    return clamp(threshold + bias, 0.0, 0.999999);
}

fn vac_threshold(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, VAC_SIZE);
    let my = rem_euclid_i32(gy, VAC_SIZE);
    return f32(ranks[u32(my) * 64u + u32(mx)]) / VAC_N;
}

fn sample_vac(gx: i32, gy: i32) -> f32 {
    let rot = rotate_with_trig(gx, gy, pat.packed.x, pat.packed.y);
    let t = vac_threshold(rot.x, rot.y);
    return apply_threshold_bias(t, pat.packed.w);
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
    let color_mode = pat.packed.z;
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
fn vac_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_vac(gx, gy));
}
