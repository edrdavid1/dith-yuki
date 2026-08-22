// Bayer on GPU-resident storage textures — separate read/write bindings (baseline WebGPU).

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct BayerUniforms {
    tile: TileUniforms,
    // x=levels, y=threshold_scale (z/w unused — keep primary uniform ≤32 B on Metal)
    params: vec4<f32>,
}

struct BayerPatternUniforms {
    // x=pattern_sin, y=pattern_cos, z=color_mode, w=threshold_bias
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: BayerUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> pat: BayerPatternUniforms;

const BAYER2: array<u32, 4> = array<u32, 4>(0u, 2u, 3u, 1u);
const BAYER4: array<u32, 16> = array<u32, 16>(
    0u, 8u, 2u, 10u,
    12u, 4u, 14u, 6u,
    3u, 11u, 1u, 9u,
    15u, 7u, 13u, 5u
);
const BAYER8: array<u32, 64> = array<u32, 64>(
    0u, 32u, 8u, 40u, 2u, 34u, 10u, 42u,
    48u, 16u, 56u, 24u, 50u, 18u, 58u, 26u,
    12u, 44u, 4u, 36u, 14u, 46u, 6u, 38u,
    60u, 28u, 52u, 20u, 62u, 30u, 54u, 22u,
    3u, 35u, 11u, 43u, 1u, 33u, 9u, 41u,
    51u, 19u, 59u, 27u, 49u, 17u, 57u, 25u,
    15u, 47u, 7u, 39u, 13u, 45u, 5u, 37u,
    63u, 31u, 55u, 23u, 61u, 29u, 53u, 21u
);

fn rem_euclid_i32(a: i32, n: i32) -> i32 {
    let r = a % n;
    if (r < 0) {
        return r + n;
    }
    return r;
}

// Metal f32 rotation can sit 1 ULP above Rust; bias floor to match CPU `dither_ordered`.
const ROT_FLOOR_BIAS: f32 = 5e-6;

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

fn bayer2_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 2);
    let my = rem_euclid_i32(gy, 2);
    return f32(BAYER2[my * 2 + mx]) / 4.0;
}

fn bayer4_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 4);
    let my = rem_euclid_i32(gy, 4);
    return f32(BAYER4[my * 4 + mx]) / 16.0;
}

fn bayer8_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 8);
    let my = rem_euclid_i32(gy, 8);
    return f32(BAYER8[my * 8 + mx]) / 64.0;
}

fn sample_bayer(gx: i32, gy: i32, matrix: u32) -> f32 {
    let rot = rotate_with_trig(gx, gy, pat.packed.x, pat.packed.y);
    var t = 0.0;
    switch (matrix) {
        case 2u: { t = bayer2_i32(rot.x, rot.y); }
        case 4u: { t = bayer4_i32(rot.x, rot.y); }
        default: { t = bayer8_i32(rot.x, rot.y); }
    }
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
fn bayer2_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 2u));
}

@compute @workgroup_size(16, 16)
fn bayer4_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 4u));
}

@compute @workgroup_size(16, 16)
fn bayer8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 8u));
}
